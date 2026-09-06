use std::fs;

use axum::http::StatusCode;
use serde_json::Value;
use tower::ServiceExt;

use super::support::{fixture, json_body, request, Fixture, TestResult};

async fn suggestions(fixture: &Fixture, prefix: &str, kind: &str) -> TestResult<Value> {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("prefix", prefix)
        .append_pair("kind", kind)
        .finish();
    let response = fixture
        .router
        .clone()
        .oneshot(fixture.session.request(
            "GET",
            &format!("/api/task-path-suggestions?{query}"),
            None,
        )?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    json_body(response).await
}

#[tokio::test]
async fn authenticated_completion_resolves_relative_and_external_paths_without_writes() -> TestResult
{
    let fixture = fixture().await?;
    let external = tempfile::tempdir()?;
    fs::write(
        external.path().join("Episode 01.MKV"),
        b"synthetic test video",
    )?;
    fs::create_dir(external.path().join("Episodes"))?;
    let prefix = external.path().join("ep");
    let input = suggestions(&fixture, prefix.to_str().ok_or("non-UTF8 path")?, "input").await?;
    assert_eq!(input["items"].as_array().ok_or("missing items")?.len(), 2);
    assert_eq!(input["items"][0]["kind"], "directory");
    assert_eq!(
        input["items"][1]["value"],
        external
            .path()
            .join("Episode 01.MKV")
            .to_str()
            .ok_or("non-UTF8 path")?
    );
    let output = suggestions(&fixture, prefix.to_str().ok_or("non-UTF8 path")?, "output").await?;
    assert_eq!(output["items"].as_array().ok_or("missing items")?.len(), 1);
    let relative = suggestions(&fixture, "input/so", "input").await?;
    assert_eq!(
        relative["items"][0]["value"],
        fixture.input.to_str().ok_or("non-UTF8 path")?
    );
    assert_eq!(fs::read_dir(external.path())?.count(), 2);
    assert!(!fixture.output.exists());
    Ok(())
}

#[tokio::test]
async fn completion_requires_authentication_and_rejects_invalid_queries() -> TestResult {
    let fixture = fixture().await?;
    let response = fixture
        .router
        .clone()
        .oneshot(request(
            "GET",
            "/api/task-path-suggestions?kind=input",
            None,
        )?)
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    for query in [
        "kind=unknown",
        "kind=input&prefix=../",
        "kind=input&prefix=data/",
        "kind=output&prefix=temp/",
        "kind=input&prefix=%00",
    ] {
        let response = fixture
            .router
            .clone()
            .oneshot(fixture.session.request(
                "GET",
                &format!("/api/task-path-suggestions?{query}"),
                None,
            )?)
            .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{query}");
    }
    let root = suggestions(&fixture, "", "input").await?;
    for item in root["items"].as_array().ok_or("missing items")? {
        let path = item["value"].as_str().ok_or("missing path")?;
        assert!(!path.ends_with(&format!("data{}", std::path::MAIN_SEPARATOR)));
        assert!(!path.ends_with(&format!("temp{}", std::path::MAIN_SEPARATOR)));
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn completion_never_follows_symlinks_or_lists_private_aliases() -> TestResult {
    let fixture = fixture().await?;
    let input = fixture.input.parent().ok_or("input parent missing")?;
    let workspace = input.parent().ok_or("workspace missing")?;
    std::os::unix::fs::symlink(workspace.join("data"), input.join("private-link"))?;
    std::os::unix::fs::symlink(&fixture.input, input.join("file-link.mkv"))?;
    let result = suggestions(&fixture, "input/", "input").await?;
    assert_eq!(result["items"].as_array().ok_or("missing items")?.len(), 1);
    let response = fixture
        .router
        .clone()
        .oneshot(fixture.session.request(
            "GET",
            "/api/task-path-suggestions?kind=input&prefix=input/private-link/",
            None,
        )?)
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    Ok(())
}

#[tokio::test]
async fn completion_caps_results_and_reports_truncation() -> TestResult {
    let fixture = fixture().await?;
    let input = fixture.input.parent().ok_or("input parent missing")?;
    for index in 0..105 {
        fs::create_dir(input.join(format!("folder-{index:03}")))?;
    }
    let result = suggestions(&fixture, "input/folder-", "output").await?;
    assert_eq!(
        result["items"].as_array().ok_or("missing items")?.len(),
        100
    );
    assert_eq!(result["truncated"], true);
    let filtered = suggestions(&fixture, "input/folder-104", "output").await?;
    assert_eq!(
        filtered["items"].as_array().ok_or("missing items")?.len(),
        1
    );
    assert_eq!(filtered["truncated"], false);
    Ok(())
}
