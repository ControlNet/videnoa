use std::fs;

use axum::http::StatusCode;
use serde_json::{json, Value};
use tower::ServiceExt;

use super::support::{fixture, json_body, Fixture, TestResult};

fn options(pattern: &str) -> Value {
    json!({"input_pattern": pattern, "output_mode": "beside_input",
        "output_directory": null, "naming_mode": "insert_extension",
        "middle_extension": "AI", "workflow": "anime-upscale", "priority": 1})
}

async fn count(fixture: &Fixture) -> TestResult<Value> {
    let response = fixture
        .router
        .clone()
        .oneshot(fixture.session.request("GET", "/api/tasks", None)?)
        .await?;
    Ok(json_body(response).await?["total"].clone())
}

#[tokio::test]
async fn brace_extensions_preview_and_create_only_matching_videos_once() -> TestResult {
    let fixture = fixture().await?;
    let root = fixture.input.parent().ok_or("missing input parent")?;
    // Synthetic files test matching and task intake, never video decoding.
    for extension in ["mkv", "mp4", "ass", "jpg"] {
        fs::write(
            root.join(format!("魔女之旅 S01E01.{extension}")),
            b"synthetic wildcard fixture",
        )?;
    }
    let body = options("input/魔女之旅 S01E01.{mkv,mp4,mkv}");
    let response = fixture
        .router
        .clone()
        .oneshot(
            fixture
                .session
                .request("POST", "/api/tasks/batch-preview", Some(&body))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let preview = json_body(response).await?;
    let rows = preview["items"].as_array().ok_or("missing items")?;
    assert_eq!(rows.len(), 2);
    assert_eq!(count(&fixture).await?, 0);
    for row in rows {
        assert!(row["error"].is_null());
        let input = row["request"]["input_path"]
            .as_str()
            .ok_or("missing input")?;
        assert!(input.ends_with("S01E01.mkv") || input.ends_with("S01E01.mp4"));
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
    let result = json_body(response).await?;
    assert_eq!(result["created"], 2);
    assert_eq!(result["failed"], 0);
    assert_eq!(count(&fixture).await?, 2);
    Ok(())
}

#[tokio::test]
async fn braces_combine_with_directories_recursion_and_character_classes() -> TestResult {
    let fixture = fixture().await?;
    let root = fixture.input.parent().ok_or("missing input parent")?;
    for season in ["Season 1", "Season 2", "Season 3"] {
        fs::create_dir_all(root.join(season).join("nested"))?;
        fs::write(
            root.join(season).join("nested/E01.mkv"),
            b"synthetic recursive fixture",
        )?;
        fs::write(
            root.join(season).join("nested/E02.mp4"),
            b"synthetic recursive fixture",
        )?;
    }
    let response = fixture
        .router
        .clone()
        .oneshot(fixture.session.request(
            "POST",
            "/api/tasks/batch-preview",
            Some(&options("input/Season {1,2}/**/{E,F}0[12].{mkv,mp4}")),
        )?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let result = json_body(response).await?;
    assert_eq!(result["items"].as_array().ok_or("missing items")?.len(), 4);
    assert_eq!(count(&fixture).await?, 0);
    Ok(())
}

#[tokio::test]
async fn invalid_or_excessive_brace_patterns_reject_without_creating_tasks() -> TestResult {
    let fixture = fixture().await?;
    let excessive = format!(
        "input/*.{{{}}}",
        (0..65).map(|n| n.to_string()).collect::<Vec<_>>().join(",")
    );
    for pattern in [
        "input/*.{mkv,mp4",
        "input/*.mkv}",
        "input/*.{mkv,}",
        "input/*.{mkv}",
        "input/*.{mkv,{mp4,avi}}",
        "input/{**,season}/*.mkv",
        "input/{../data,season}/*.mkv",
        excessive.as_str(),
    ] {
        for route in ["/api/tasks/batch-preview", "/api/tasks/batch"] {
            let response = fixture
                .router
                .clone()
                .oneshot(
                    fixture
                        .session
                        .request("POST", route, Some(&options(pattern)))?,
                )
                .await?;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{pattern}");
        }
    }
    assert_eq!(count(&fixture).await?, 0);
    Ok(())
}

#[tokio::test]
async fn brace_matching_preserves_the_all_or_nothing_preview_gate() -> TestResult {
    let fixture = fixture().await?;
    let root = fixture.input.parent().ok_or("missing input parent")?;
    for name in ["E01.mkv", "E01.mp4", "E01.AI.mp4"] {
        fs::write(root.join(name), b"synthetic existing output fixture")?;
    }
    let response = fixture
        .router
        .clone()
        .oneshot(fixture.session.request(
            "POST",
            "/api/tasks/batch",
            Some(&options("input/E01.{mkv,mp4}")),
        )?)
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await?["created"], 0);
    assert_eq!(count(&fixture).await?, 0);
    Ok(())
}

#[tokio::test]
async fn brace_alternatives_share_the_total_file_limit() -> TestResult {
    let fixture = fixture().await?;
    let root = fixture.input.parent().ok_or("missing input parent")?;
    for index in 0..501 {
        let extension = if index < 250 { "mkv" } else { "mp4" };
        fs::write(
            root.join(format!("E{index}.{extension}")),
            b"synthetic combined limit fixture",
        )?;
    }
    let response = fixture
        .router
        .clone()
        .oneshot(fixture.session.request(
            "POST",
            "/api/tasks/batch",
            Some(&options("input/E*.{mkv,mp4}")),
        )?)
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        json_body(response).await?["error"]["field_errors"][0]["message"]
            .as_str()
            .ok_or("missing limit error")?
            .contains("500")
    );
    assert_eq!(count(&fixture).await?, 0);
    Ok(())
}
