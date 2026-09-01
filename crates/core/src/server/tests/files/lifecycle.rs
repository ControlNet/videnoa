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
