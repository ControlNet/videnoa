use super::*;

#[test]
fn test_files_workspace_is_created() {
    let temp = temp_dir();

    let _app = test_app(temp.path());

    assert!(temp.path().join("workspace").is_dir());
}

#[tokio::test]
async fn test_files_upload_streams_nested_file() {
    let temp = temp_dir();
    let app = test_app(temp.path());
    let original = b"streamed-video-payload";

    let response = send(
        &app,
        Method::PUT,
        "/api/files/nested/path/input.mkv",
        Body::from(original.as_slice()),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let response_body = body_bytes(response).await;
    let metadata: serde_json::Value =
        serde_json::from_slice(&response_body).expect("parse upload metadata");
    assert_workflow_path(
        metadata["path"].as_str().expect("upload response path"),
        &workspace_path(temp.path(), "nested/path/input.mkv"),
    );
    assert_eq!(metadata["size"], original.len());
    assert_eq!(
        std::fs::read(workspace_path(temp.path(), "nested/path/input.mkv"))
            .expect("read uploaded file"),
        original
    );
}

#[tokio::test]
async fn interrupted_upload_keeps_the_existing_file() {
    // Given: a file already uploaded at the target path.
    let temp = temp_dir();
    create_workspace_file(
        temp.path(),
        "task-123/input.mkv",
        b"complete earlier upload",
    );
    let app = test_app(temp.path());

    // When: a replacement upload breaks off partway through its body.
    let body = Body::from_stream(futures_util::stream::iter([
        Ok(axum::body::Bytes::from_static(b"partial")),
        Err(std::io::Error::other("client went away")),
    ]));
    let response = send(&app, Method::PUT, "/api/files/task-123/input.mkv", body).await;

    // Then: the request fails, the earlier file is intact, and no staging
    // file is left beside it.
    assert_ne!(response.status(), StatusCode::OK);
    assert_eq!(
        std::fs::read(workspace_path(temp.path(), "task-123/input.mkv")).unwrap(),
        b"complete earlier upload"
    );
    let names: Vec<String> = std::fs::read_dir(workspace_path(temp.path(), "task-123"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["input.mkv"]);
}

#[tokio::test]
async fn upload_replaces_an_existing_file_without_leaving_staging_files() {
    let temp = temp_dir();
    create_workspace_file(temp.path(), "task-123/input.mkv", b"old");
    let app = test_app(temp.path());

    let response = send(
        &app,
        Method::PUT,
        "/api/files/task-123/input.mkv",
        Body::from("new contents"),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        std::fs::read(workspace_path(temp.path(), "task-123/input.mkv")).unwrap(),
        b"new contents"
    );
    let names: Vec<String> = std::fs::read_dir(workspace_path(temp.path(), "task-123"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["input.mkv"]);

    // The upload gets the permissions of any new file, not the owner-only
    // mode of a temp file.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let reference = workspace_path(temp.path(), "task-123/reference");
        std::fs::write(&reference, b"").unwrap();
        let mode =
            |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode(&workspace_path(temp.path(), "task-123/input.mkv")),
            mode(&reference)
        );
    }
}

#[tokio::test]
async fn test_files_download_returns_original_bytes() {
    let temp = temp_dir();
    let original = b"generated-video-payload";
    create_workspace_file(temp.path(), "task-123/output.mkv", original);
    let app = test_app(temp.path());

    let response = send(
        &app,
        Method::GET,
        "/api/files/task-123/output.mkv",
        Body::empty(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[axum::http::header::CONTENT_LENGTH],
        original.len().to_string()
    );
    assert_eq!(body_bytes(response).await.as_ref(), original);
}

#[tokio::test]
async fn test_files_stat_reports_size_and_type() {
    let temp = temp_dir();
    let original = b"output";
    create_workspace_file(temp.path(), "task-123/output.mkv", original);
    let app = test_app(temp.path());

    let response = send(
        &app,
        Method::GET,
        "/api/files/task-123/output.mkv/stat",
        Body::empty(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let response_body = body_bytes(response).await;
    let metadata: serde_json::Value =
        serde_json::from_slice(&response_body).expect("parse stat metadata");
    assert_workflow_path(
        metadata["path"].as_str().expect("stat response path"),
        &workspace_path(temp.path(), "task-123/output.mkv"),
    );
    assert_eq!(metadata["size"], original.len());
    assert_eq!(metadata["is_file"], true);
    assert_eq!(metadata["is_dir"], false);
}

#[tokio::test]
async fn test_files_delete_file() {
    let temp = temp_dir();
    create_workspace_file(temp.path(), "task-123/output.mkv", b"output");
    let app = test_app(temp.path());

    let response = send(
        &app,
        Method::DELETE,
        "/api/files/task-123/output.mkv",
        Body::empty(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(!workspace_path(temp.path(), "task-123/output.mkv").exists());
}

#[tokio::test]
async fn test_files_delete_directory_recursively() {
    let temp = temp_dir();
    create_workspace_file(temp.path(), "task-123/input.mkv", b"input");
    create_workspace_file(temp.path(), "task-123/nested/output.mkv", b"output");
    let app = test_app(temp.path());

    let response = send(&app, Method::DELETE, "/api/files/task-123", Body::empty()).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(!workspace_path(temp.path(), "task-123").exists());
}
