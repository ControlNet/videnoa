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
async fn batch_preview_resolves_links_deduplicates_targets_and_skips_private_storage() -> TestResult
{
    let fixture = fixture().await?;
    let root = fixture.input.parent().ok_or("missing parent")?;
    let workspace = root.parent().ok_or("missing workspace")?;
    fs::write(
        workspace.join("data/private.MKV"),
        b"synthetic private fixture",
    )?;
    std::os::unix::fs::symlink(workspace.join("data"), root.join("alias"))?;
    std::os::unix::fs::symlink(&fixture.input, root.join("linked.MKV"))?;
    std::os::unix::fs::symlink(root, root.join("cycle"))?;
    let result = preview(&fixture, &options("**/*.MKV")).await?;
    assert_eq!(result["items"].as_array().ok_or("missing items")?.len(), 1);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn batch_creation_accepts_a_linked_media_directory_and_persists_real_paths() -> TestResult {
    let fixture = fixture().await?;
    let root = fixture.input.parent().ok_or("input parent")?;
    let workspace = root.parent().ok_or("workspace")?;
    let actual = workspace.join("[Media]{season}");
    fs::create_dir(&actual)?;
    fs::copy(&fixture.input, actual.join("source.MKV"))?;
    std::os::unix::fs::symlink("[Media]{season}", workspace.join("Bangumi"))?;
    let response = fixture
        .router
        .clone()
        .oneshot(fixture.session.request(
            "POST",
            "/api/tasks/batch",
            Some(&options("Bangumi/*.MKV")),
        )?)
        .await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = json_body(response).await?;
    assert_eq!(body["created"], 1);
    assert_eq!(
        body["items"][0]["task"]["input_path"],
        actual.join("source.MKV").to_str().ok_or("input path")?
    );
    assert_eq!(
        body["items"][0]["task"]["output_path"],
        actual.join("source.AI.MKV").to_str().ok_or("output path")?
    );
    Ok(())
}

#[tokio::test]
async fn jellyfin_suffix_preview_and_batch_create_preserve_paths() -> TestResult {
    let fixture = fixture().await?;
    let media = tempfile::tempdir()?;
    // Synthetic intake files; no decoding or real media jobs are involved.
    for name in ["Re Zero S03E01.mkv", "动画.S03E02 - Original.MKV"] {
        fs::write(media.path().join(name), b"synthetic suffix intake fixture")?;
    }
    let mut body = options(&format!("{}/*", media.path().display()));
    body["naming_mode"] = json!("jellyfin_version_suffix");
    for (mode, label) in [("beside_input", "AI"), ("directory", "AI 4K")] {
        body["output_mode"] = json!(mode);
        body["output_directory"] = json!("output");
        body["middle_extension"] = json!(label);
        let result = preview(&fixture, &body).await?;
        let items = result["items"].as_array().ok_or("missing items")?;
        assert_eq!(items.len(), 2);
        for item in items {
            assert!(item["error"].is_null());
            let input =
                std::path::Path::new(item["request"]["input_path"].as_str().ok_or("input")?);
            let directory = if mode == "beside_input" {
                media.path()
            } else {
                fixture.output.parent().ok_or("output parent")?
            };
            let expected = directory.join(format!(
                "{} - {label}.{}",
                input.file_stem().ok_or("stem")?.to_string_lossy(),
                input.extension().ok_or("extension")?.to_string_lossy()
            ));
            assert_eq!(
                item["request"]["output_path"],
                expected.to_str().ok_or("UTF8")?
            );
        }
        let response = fixture
            .router
            .clone()
            .oneshot(
                fixture
                    .session
                    .request("POST", "/api/tasks/batch", Some(&body))?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::CREATED);
        let created = json_body(response).await?;
        assert_eq!(created["created"], 2);
        for (index, item) in items.iter().enumerate() {
            assert_eq!(
                created["items"][index]["task"]["output_path"],
                item["request"]["output_path"]
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn jellyfin_suffix_rejects_invalid_labels_and_existing_outputs() -> TestResult {
    let fixture = fixture().await?;
    let mut body = options("input/*.MKV");
    body["naming_mode"] = json!("jellyfin_version_suffix");
    for label in [
        String::new(),
        " ".to_owned(),
        " AI".to_owned(),
        "AI ".to_owned(),
        "../AI".to_owned(),
        "AI\\4K".to_owned(),
        "AI?".to_owned(),
        "AI\n".to_owned(),
        ".AI".to_owned(),
        "AI.".to_owned(),
        "界".repeat(22),
    ] {
        body["middle_extension"] = json!(label);
        for route in ["/api/tasks/batch-preview", "/api/tasks/batch"] {
            let response = fixture
                .router
                .clone()
                .oneshot(fixture.session.request("POST", route, Some(&body))?)
                .await?;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(
                json_body(response).await?["error"]["field_errors"][0]["field"],
                "middle_extension"
            );
        }
    }
    body["middle_extension"] = json!("AI");
    fs::copy(
        &fixture.input,
        fixture.input.with_file_name("source - AI.MKV"),
    )?;
    body["input_pattern"] = json!("input/source.MKV");
    let result = preview(&fixture, &body).await?;
    assert_eq!(
        result["items"][0]["validation_error"],
        "Output already exists and will not be overwritten."
    );
    let response = fixture
        .router
        .clone()
        .oneshot(
            fixture
                .session
                .request("POST", "/api/tasks/batch", Some(&body))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await?["created"], 0);

    let nested = fixture.input.parent().ok_or("input parent")?.join("season");
    fs::create_dir(&nested)?;
    fs::copy(&fixture.input, nested.join("source.MKV"))?;
    body["input_pattern"] = json!("input/**/source.MKV");
    body["output_mode"] = json!("directory");
    body["output_directory"] = json!("output");
    let result = preview(&fixture, &body).await?;
    let items = result["items"].as_array().ok_or("items")?;
    assert_eq!(items.len(), 2);
    for item in items {
        assert_eq!(item["error"], "Multiple inputs map to this output path.");
        assert_eq!(item["output_key"], items[0]["output_key"]);
    }
    Ok(())
}
