use std::fs;

use axum::http::StatusCode;
use serde_json::{json, Value};
use tower::ServiceExt;

use super::support::{fixture, json_body, request, Fixture, TestResult};

fn options(pattern: &str) -> Value {
    json!({"input_pattern": pattern, "output_mode": "beside_input",
        "output_directory": null, "naming_mode": "insert_extension",
        "middle_extension": "AI", "workflow": "anime-upscale", "priority": 7})
}

async fn preview(fixture: &Fixture, options: &Value) -> TestResult<Value> {
    let response = fixture
        .router
        .clone()
        .oneshot(
            fixture
                .session
                .request("POST", "/api/tasks/batch-preview", Some(options))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

#[tokio::test]
async fn batch_preview_matches_recursive_external_files_and_creates_exact_preview() -> TestResult {
    let fixture = fixture().await?;
    let media = tempfile::tempdir()?;
    fs::create_dir(media.path().join("season"))?;
    // Synthetic file contents test intake only; no video decoding is performed.
    for name in ["E01.mkv", "season/E02.mkv", "ignore.txt"] {
        fs::write(media.path().join(name), b"synthetic batch intake fixture")?;
    }
    let pattern = format!("{}/**/*.mkv", media.path().display());
    let result = preview(&fixture, &options(&pattern)).await?;
    let items = result["items"].as_array().ok_or("missing items")?;
    assert_eq!(items.len(), 2);
    assert_eq!(fs::read_dir(media.path())?.count(), 3);
    for (index, item) in items.iter().enumerate() {
        assert!(item["error"].is_null());
        assert!(item["request"]["output_path"]
            .as_str()
            .ok_or("missing output")?
            .ends_with(".AI.mkv"));
        for expected in [StatusCode::CREATED, StatusCode::OK] {
            let mut request =
                fixture
                    .session
                    .request("POST", "/api/tasks", Some(&item["request"]))?;
            request
                .headers_mut()
                .insert("idempotency-key", format!("batch-{index}").parse()?);
            let response = fixture.router.clone().oneshot(request).await?;
            assert_eq!(response.status(), expected);
        }
    }
    let response = fixture
        .router
        .clone()
        .oneshot(fixture.session.request("GET", "/api/tasks", None)?)
        .await?;
    assert_eq!(json_body(response).await?["total"], 2);
    Ok(())
}

#[tokio::test]
async fn batch_preview_reports_collisions_existing_outputs_and_original_names() -> TestResult {
    let fixture = fixture().await?;
    let root = fixture.input.parent().ok_or("missing parent")?;
    fs::create_dir(root.join("season"))?;
    fs::copy(&fixture.input, root.join("season/source.MKV"))?;
    let mut request = options("input/**/*.MKV");
    request["output_mode"] = json!("directory");
    request["output_directory"] = json!("output");
    let result = preview(&fixture, &request).await?;
    for row in result["items"].as_array().ok_or("missing items")? {
        assert_eq!(row["error"], "Multiple inputs map to this output path.");
        assert!(row["validation_error"].is_null());
        assert_eq!(row["output_key"], result["items"][0]["output_key"]);
    }
    let duplicate_output = fixture
        .output
        .parent()
        .ok_or("missing output parent")?
        .join("source.AI.MKV");
    fs::copy(&fixture.input, &duplicate_output)?;
    let occupied = preview(&fixture, &request).await?;
    for row in occupied["items"].as_array().ok_or("missing items")? {
        assert_eq!(row["error"], "Multiple inputs map to this output path.");
        assert_eq!(
            row["validation_error"],
            "Output already exists and will not be overwritten."
        );
    }
    request["input_pattern"] = json!("input/*.MKV");
    request["naming_mode"] = json!("original");
    let result = preview(&fixture, &request).await?;
    assert!(result["items"][0]["error"].is_null());
    assert_eq!(
        result["items"][0]["request"]["output_path"],
        root.parent()
            .ok_or("missing workspace")?
            .join("output/source.MKV")
            .to_str()
            .ok_or("non-UTF8")?
    );
    fs::copy(
        &fixture.input,
        fixture
            .output
            .parent()
            .ok_or("missing output parent")?
            .join("source.MKV"),
    )?;
    let result = preview(&fixture, &request).await?;
    assert_eq!(
        result["items"][0]["error"],
        "Output already exists and will not be overwritten."
    );
    Ok(())
}

#[tokio::test]
async fn batch_preview_validates_options_auth_and_limits() -> TestResult {
    let fixture = fixture().await?;
    let response = fixture
        .router
        .clone()
        .oneshot(request(
            "POST",
            "/api/tasks/batch-preview",
            Some(&options("input/*")),
        )?)
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let mut no_csrf = fixture.session.request(
        "POST",
        "/api/tasks/batch-preview",
        Some(&options("input/*")),
    )?;
    no_csrf.headers_mut().remove("x-csrf-token");
    assert_eq!(
        fixture.router.clone().oneshot(no_csrf).await?.status(),
        StatusCode::FORBIDDEN
    );
    for (field, value) in [
        ("input_pattern", json!("../*")),
        ("input_pattern", json!("data/*")),
        ("input_pattern", json!("input/[")),
        ("naming_mode", json!("original")),
        ("middle_extension", json!("../escape")),
        ("priority", json!(101)),
        ("workflow", json!("")),
        ("output_mode", json!("directory")),
    ] {
        let mut body = options("input/*");
        body[field] = value;
        let response = fixture
            .router
            .clone()
            .oneshot(
                fixture
                    .session
                    .request("POST", "/api/tasks/batch-preview", Some(&body))?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{field}");
    }
    assert_eq!(
        preview(&fixture, &options("input/absent*.mkv")).await?["items"],
        json!([])
    );
    let root = fixture.input.parent().ok_or("missing parent")?;
    for index in 0..501 {
        fs::write(
            root.join(format!("{index}.mkv")),
            b"synthetic limit fixture",
        )?;
    }
    let response = fixture
        .router
        .clone()
        .oneshot(fixture.session.request(
            "POST",
            "/api/tasks/batch-preview",
            Some(&options("input/*.mkv")),
        )?)
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn batch_preview_skips_private_storage_and_symlinks() -> TestResult {
    let fixture = fixture().await?;
    let root = fixture.input.parent().ok_or("missing parent")?;
    let workspace = root.parent().ok_or("missing workspace")?;
    fs::write(
        workspace.join("data/private.MKV"),
        b"synthetic private fixture",
    )?;
    std::os::unix::fs::symlink(workspace.join("data"), root.join("alias"))?;
    std::os::unix::fs::symlink(&fixture.input, root.join("linked.MKV"))?;
    let result = preview(&fixture, &options("**/*.MKV")).await?;
    assert_eq!(result["items"].as_array().ok_or("missing items")?.len(), 1);
    Ok(())
}
