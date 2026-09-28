//! Isolated process: this one test changes CWD to install a synthetic FFmpeg fault
//! injector. Keep it separate from other tests that resolve executables via CWD.
#![cfg(unix)]
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use tempfile::TempDir;
use tower::ServiceExt;
use videnoa_core::{
    config::AppConfig,
    server::{api_router, app_state_with_config},
};

fn app(dir: &TempDir) -> Router {
    api_router(app_state_with_config(
        AppConfig::default(),
        dir.path().join("config.toml"),
        dir.path().join("data"),
    ))
}

async fn request(app: &Router, method: &str, uri: &str, value: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(value.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn video(dir: &Path) -> PathBuf {
    let path = dir.join("input.mkv");
    assert!(Command::new("/usr/bin/ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=32x32:rate=2",
            "-frames:v",
            "10",
            "-c:v",
            "ffv1"
        ])
        .arg(&path)
        .status()
        .unwrap()
        .success());
    path
}

struct CurrentDirectory(PathBuf);
impl Drop for CurrentDirectory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).unwrap();
    }
}
fn enter(dir: &Path) -> CurrentDirectory {
    let previous = CurrentDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(dir).unwrap();
    previous
}
#[cfg(unix)]
fn executable(path: &Path, contents: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, contents).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_decoder_job_is_reported_failed_after_emitting_a_real_frame() {
    if !Path::new("/usr/bin/ffmpeg").exists() || !Path::new("/usr/bin/python3").exists() {
        eprintln!("Skipping media fault injection: system FFmpeg and Python are required");
        return;
    }
    let dir = TempDir::new().unwrap();
    let input = video(dir.path());
    let output = dir.path().join("output.mkv");
    // Fault injection only: decode one real frame then exit 7. Encoding still
    // invokes unmodified system FFmpeg, so the output is a real video file.
    executable(&dir.path().join("ffmpeg"), "#!/usr/bin/python3\nimport os, subprocess, sys\na=sys.argv[1:]\nif a[-1]=='pipe:1':\n r=subprocess.run(['/usr/bin/ffmpeg',*a[:-1],'-frames:v','1',a[-1]])\n sys.exit(7 if r.returncode==0 else r.returncode)\nos.execv('/usr/bin/ffmpeg',['ffmpeg',*a])\n");
    let _cwd = enter(dir.path());
    let app = app(&dir);
    let workflow = json!({"nodes":[
        {"id":"input","node_type":"VideoInput","params":{"path":input}},
        {"id":"output","node_type":"VideoOutput","params":{"output_path":output,"codec":"libx264","pixel_format":"yuv420p","x265_preset":"ultrafast"}}
    ],"connections":[
        {"from_node":"input","from_port":"frames","to_node":"output","to_port":"frames","port_type":"VideoFrames"},
        {"from_node":"input","from_port":"source_path","to_node":"output","to_port":"source_path","port_type":"Path"}
    ]});
    let (status, created) = request(&app, "POST", "/api/jobs", json!({"workflow":workflow})).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let job = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let (_, job) = request(
                &app,
                "GET",
                &format!("/api/jobs/{}", created["id"].as_str().unwrap()),
                Value::Null,
            )
            .await;
            if matches!(
                job["status"].as_str(),
                Some("completed" | "failed" | "cancelled")
            ) {
                break job;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(job["status"], "failed", "{job}");
    assert!(job["error"].as_str().unwrap().contains("decode"), "{job}");
}
