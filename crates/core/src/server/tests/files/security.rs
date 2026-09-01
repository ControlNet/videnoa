use super::*;

#[tokio::test]
async fn test_files_rejects_parent_traversal() {
    let temp = temp_dir();
    let app = test_app(temp.path());

    for uri in [
        "/api/files/..%2Foutside.bin",
        "/api/files/task%2F..%2F..%2Foutside.bin",
    ] {
        let response = send(&app, Method::PUT, uri, Body::from("blocked")).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "URI: {uri}");
    }
}

#[tokio::test]
async fn test_files_rejects_absolute_paths() {
    let temp = temp_dir();
    let app = test_app(temp.path());

    for uri in [
        "/api/files/%2Fetc%2Fpasswd",
        "/api/files/C:%5CWindows%5Cwin.ini",
        "/api/files/~%2Fsecret",
    ] {
        let response = send(&app, Method::GET, uri, Body::empty()).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "URI: {uri}");
    }
}

#[tokio::test]
async fn test_files_rejects_workspace_root_delete() {
    let temp = temp_dir();
    let app = test_app(temp.path());

    let response = send(&app, Method::DELETE, "/api/files", Body::empty()).await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(temp.path().join("workspace").is_dir());
}

#[cfg(unix)]
#[tokio::test]
async fn test_files_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let temp = temp_dir();
    let outside = temp.path().join("outside.bin");
    std::fs::write(&outside, b"outside").expect("write outside fixture");
    std::fs::create_dir_all(temp.path().join("workspace")).expect("create workspace");
    symlink(&outside, temp.path().join("workspace/link.bin")).expect("create escape symlink");
    let app = test_app(temp.path());

    let response = send(
        &app,
        Method::PUT,
        "/api/files/link.bin",
        Body::from("overwritten"),
    )
    .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        std::fs::read(outside).expect("read outside fixture"),
        b"outside"
    );
}

#[tokio::test]
async fn test_files_cannot_access_outside_workspace() {
    let temp = temp_dir();
    let outside = temp.path().join("outside.bin");
    std::fs::write(&outside, b"outside-secret").expect("write outside fixture");
    let app = test_app(temp.path());
    let encoded_absolute = format!(
        "/api/files/%2F{}",
        outside
            .strip_prefix("/")
            .expect("absolute temp path")
            .to_string_lossy()
            .replace('/', "%2F")
    );

    let response = send(&app, Method::GET, &encoded_absolute, Body::empty()).await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_ne!(body_bytes(response).await.as_ref(), b"outside-secret");
}

#[tokio::test]
async fn test_files_routes_do_not_restrict_existing_fs_routes() {
    let temp = temp_dir();
    let models_dir = temp.path().join("outside-models");
    std::fs::create_dir_all(&models_dir).expect("create models fixture");
    std::fs::write(models_dir.join("model.onnx"), b"model").expect("write model fixture");
    let app = test_app_with_models(temp.path(), &models_dir);

    let browse_uri = format!("/api/fs/browse?path={}", models_dir.display());
    let browse_response = send(&app, Method::GET, &browse_uri, Body::empty()).await;
    assert_eq!(browse_response.status(), StatusCode::OK);
    let browse_entries: Vec<FsEntry> =
        serde_json::from_slice(&body_bytes(browse_response).await).expect("parse browse entries");
    assert!(browse_entries
        .iter()
        .any(|entry| entry.name == "model.onnx"));

    let list_response = send(&app, Method::GET, "/api/fs/list?base=models", Body::empty()).await;
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_entries: Vec<FsEntry> =
        serde_json::from_slice(&body_bytes(list_response).await).expect("parse list entries");
    assert!(list_entries.iter().any(|entry| entry.name == "model.onnx"));
}
