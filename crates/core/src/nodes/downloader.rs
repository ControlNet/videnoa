use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};
use tracing::debug;
use url::Url;

use crate::node::{ExecutionContext, Node, PortDefinition};
use crate::types::{PortData, PortType};

pub struct DownloaderNode;

const DOWNLOAD_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_REQUEST_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const DOWNLOAD_MAX_ATTEMPTS: usize = 3;
const DOWNLOAD_RETRY_BACKOFF_MS: u64 = 250;

impl DownloaderNode {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DownloaderNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for DownloaderNode {
    fn node_type(&self) -> &str {
        "downloader"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![PortDefinition {
            name: "url".to_string(),
            port_type: PortType::Str,
            required: true,
            default_value: None,
        }]
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![PortDefinition {
            name: "path".to_string(),
            port_type: PortType::Path,
            required: true,
            default_value: None,
        }]
    }

    fn execute(
        &mut self,
        inputs: &HashMap<String, PortData>,
        ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        let url_raw = match inputs.get("url") {
            Some(PortData::Str(value)) => value,
            _ => bail!("missing or invalid 'url' input (expected Str)"),
        };

        let parsed_url = parse_http_url(url_raw)?;
        let redacted = redacted_url_for_display(&parsed_url);
        debug!(url = %redacted, "downloading URL to local path");
        // One directory per download keeps the original file name without
        // colliding with other downloads; it is removed when the job ends.
        let download_dir = ctx.scratch.allocate_dir()?;
        let final_path = download_to_file(&parsed_url, &redacted, &download_dir)?;

        let mut outputs = HashMap::new();
        outputs.insert("path".to_string(), PortData::Path(final_path));
        Ok(outputs)
    }
}

fn parse_http_url(raw: &str) -> Result<Url> {
    let parsed = Url::parse(raw).with_context(|| {
        format!(
            "invalid downloader URL: {}",
            crate::logging::redact_sensitive_text(raw)
        )
    })?;

    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        scheme => {
            let redacted = redacted_url_for_display(&parsed);
            bail!(
                "unsupported downloader URL scheme '{scheme}' for '{redacted}' (expected http/https)"
            )
        }
    }
}

fn download_to_file(url: &Url, redacted_url: &str, download_dir: &Path) -> Result<PathBuf> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(DOWNLOAD_CONNECT_TIMEOUT)
        .timeout(DOWNLOAD_REQUEST_TIMEOUT)
        .build()
        .context("failed to build HTTP client for downloader")?;

    for attempt in 1..=DOWNLOAD_MAX_ATTEMPTS {
        match download_once(&client, url, redacted_url, download_dir) {
            Ok(final_path) => return Ok(final_path),
            Err(attempt_error) => {
                let DownloadAttemptError { retryable, error } = attempt_error;

                if retryable && attempt < DOWNLOAD_MAX_ATTEMPTS {
                    debug!(
                        url = %redacted_url,
                        attempt,
                        max_attempts = DOWNLOAD_MAX_ATTEMPTS,
                        error = %error,
                        "download attempt failed; retrying"
                    );

                    let backoff_ms = DOWNLOAD_RETRY_BACKOFF_MS.saturating_mul(attempt as u64);
                    std::thread::sleep(Duration::from_millis(backoff_ms));
                    continue;
                }

                if retryable {
                    return Err(anyhow!(
                        "download failed after {} attempts for {}: {}",
                        DOWNLOAD_MAX_ATTEMPTS,
                        redacted_url,
                        error
                    ));
                }

                return Err(error);
            }
        }
    }

    Err(anyhow!(
        "download failed after {} attempts for {}",
        DOWNLOAD_MAX_ATTEMPTS,
        redacted_url
    ))
}

fn download_once(
    client: &reqwest::blocking::Client,
    url: &Url,
    redacted_url: &str,
    download_dir: &Path,
) -> std::result::Result<PathBuf, DownloadAttemptError> {
    let mut response = client.get(url.as_str()).send().map_err(|err| {
        let wrapped = anyhow!("failed to start download from {redacted_url}");
        if is_retryable_reqwest_error(&err) {
            DownloadAttemptError::retryable(wrapped)
        } else {
            DownloadAttemptError::fatal(wrapped)
        }
    })?;

    if !response.status().is_success() {
        let status = response.status();
        let wrapped = anyhow!(
            "download request returned HTTP {} for {}",
            status.as_u16(),
            redacted_url
        );

        if is_retryable_status(status) {
            return Err(DownloadAttemptError::retryable(wrapped));
        }
        return Err(DownloadAttemptError::fatal(wrapped));
    }

    let (final_path, tmp_path) =
        destination_paths_for_url_and_headers(download_dir, url, Some(response.headers()));

    cleanup_file_if_exists(&tmp_path);

    let mut tmp_file = fs::File::create(&tmp_path)
        .with_context(|| format!("failed to create temp file: {}", tmp_path.display()))
        .map_err(DownloadAttemptError::fatal)?;
    let mut tmp_guard = TempFileCleanupGuard::new(&tmp_path);

    response.copy_to(&mut tmp_file).map_err(|err| {
        let wrapped = anyhow!("failed while reading HTTP body from {redacted_url}");
        if is_retryable_reqwest_error(&err) {
            DownloadAttemptError::retryable(wrapped)
        } else {
            DownloadAttemptError::fatal(wrapped)
        }
    })?;

    tmp_file
        .flush()
        .with_context(|| format!("failed to flush temp file: {}", tmp_path.display()))
        .map_err(DownloadAttemptError::fatal)?;

    tmp_file
        .sync_all()
        .with_context(|| format!("failed to fsync temp file: {}", tmp_path.display()))
        .map_err(DownloadAttemptError::fatal)?;

    drop(tmp_file);

    fs::rename(&tmp_path, &final_path)
        .with_context(|| {
            format!(
                "failed to atomically move {} -> {}",
                tmp_path.display(),
                final_path.display()
            )
        })
        .map_err(DownloadAttemptError::fatal)?;

    tmp_guard.disarm();
    Ok(final_path)
}

fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    status.as_u16() == 408 || status.as_u16() == 429 || status.is_server_error()
}

fn is_retryable_reqwest_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request() || err.is_body()
}

struct DownloadAttemptError {
    retryable: bool,
    error: anyhow::Error,
}

impl DownloadAttemptError {
    fn retryable(error: anyhow::Error) -> Self {
        Self {
            retryable: true,
            error,
        }
    }

    fn fatal(error: anyhow::Error) -> Self {
        Self {
            retryable: false,
            error,
        }
    }
}

struct TempFileCleanupGuard<'a> {
    path: &'a Path,
    keep: bool,
}

impl<'a> TempFileCleanupGuard<'a> {
    fn new(path: &'a Path) -> Self {
        Self { path, keep: false }
    }

    fn disarm(&mut self) {
        self.keep = true;
    }
}

impl Drop for TempFileCleanupGuard<'_> {
    fn drop(&mut self) {
        if !self.keep {
            cleanup_file_if_exists(self.path);
        }
    }
}

fn cleanup_file_if_exists(path: &Path) {
    let _ = fs::remove_file(path);
}

fn destination_paths_for_url_and_headers(
    download_dir: &Path,
    url: &Url,
    response_headers: Option<&reqwest::header::HeaderMap>,
) -> (PathBuf, PathBuf) {
    let digest = Sha256::digest(url.as_str().as_bytes());
    let digest_hex = format!("{digest:x}");
    let filename = choose_download_filename(url, &digest_hex, response_headers);

    let final_path = download_dir.join(filename);
    let tmp_path = part_path(&final_path);

    (final_path, tmp_path)
}

/// Temporary file a download is written to before it is renamed into place.
fn part_path(final_path: &Path) -> PathBuf {
    final_path.with_extension(format!(
        "{}part",
        final_path
            .extension()
            .map(|ext| format!("{}.", ext.to_string_lossy()))
            .unwrap_or_default()
    ))
}

fn choose_download_filename(
    url: &Url,
    digest_hex: &str,
    response_headers: Option<&reqwest::header::HeaderMap>,
) -> String {
    let candidate = response_headers
        .and_then(filename_from_content_disposition)
        .or_else(|| filename_from_url_basename(url));

    if let Some(candidate) = candidate {
        return candidate;
    }

    deterministic_fallback_filename(url, digest_hex)
}

fn filename_from_content_disposition(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let raw = headers
        .get(reqwest::header::CONTENT_DISPOSITION)?
        .to_str()
        .ok()?;

    let mut filename_star_raw: Option<String> = None;
    let mut filename_raw: Option<String> = None;

    for (name, value) in parse_content_disposition_parameters(raw) {
        if name.eq_ignore_ascii_case("filename*") && filename_star_raw.is_none() {
            filename_star_raw = Some(value);
        } else if name.eq_ignore_ascii_case("filename") && filename_raw.is_none() {
            filename_raw = Some(value);
        }
    }

    if let Some(decoded) = filename_star_raw
        .as_deref()
        .and_then(decode_rfc8187_filename)
        .and_then(|v| sanitize_filename_candidate(v.as_str()))
    {
        return Some(decoded);
    }

    filename_raw
        .as_deref()
        .map(unquote_header_value)
        .and_then(|v| sanitize_filename_candidate(v.as_str()))
}

fn filename_from_url_basename(url: &Url) -> Option<String> {
    let raw = url
        .path_segments()
        .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))?;

    let decoded = percent_decode(raw)
        .map(|bytes| String::from_utf8_lossy(bytes.as_slice()).into_owned())
        .unwrap_or_else(|| raw.to_string());

    sanitize_filename_candidate(&decoded)
}

fn deterministic_fallback_filename(url: &Url, digest_hex: &str) -> String {
    let extension = extension_from_url(url);
    let mut filename = format!("video-{digest_hex}");
    if let Some(ext) = extension {
        filename.push('.');
        filename.push_str(ext.as_str());
    }
    filename
}

fn sanitize_filename_candidate(raw: &str) -> Option<String> {
    let no_controls: String = raw.chars().filter(|ch| !ch.is_control()).collect();
    let normalized = no_controls.replace('\\', "/");
    let leaf = normalized
        .split('/')
        .rfind(|segment| !segment.is_empty() && *segment != "." && *segment != "..")
        .unwrap_or("");

    let mut cleaned = String::with_capacity(leaf.len());
    for ch in leaf.chars() {
        if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            continue;
        }
        cleaned.push(ch);
    }

    let cleaned = cleaned.trim().trim_matches('.').chars().collect::<String>();

    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return None;
    }

    Some(cleaned)
}

fn parse_content_disposition_parameters(raw: &str) -> Vec<(String, String)> {
    split_header_parts(raw)
        .into_iter()
        .skip(1)
        .filter_map(|part| {
            let (name, value) = part.split_once('=')?;
            Some((name.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

fn split_header_parts(raw: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut in_quotes = false;
    let mut escaped = false;
    let mut start = 0usize;

    for (idx, ch) in raw.char_indices() {
        if in_quotes {
            if escaped {
                escaped = false;
                continue;
            }

            if ch == '\\' {
                escaped = true;
                continue;
            }

            if ch == '"' {
                in_quotes = false;
            }
            continue;
        }

        if ch == '"' {
            in_quotes = true;
            continue;
        }

        if ch == ';' {
            parts.push(raw[start..idx].trim());
            start = idx + 1;
        }
    }

    parts.push(raw[start..].trim());
    parts
}

fn unquote_header_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if !(trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2) {
        return trimmed.to_string();
    }

    let inner = &trimmed[1..trimmed.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut escaped = false;
    for ch in inner.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        out.push(ch);
    }

    if escaped {
        out.push('\\');
    }

    out
}

fn decode_rfc8187_filename(raw: &str) -> Option<String> {
    let value = unquote_header_value(raw);
    let mut parts = value.splitn(3, '\'');
    let charset = parts.next()?.trim().to_ascii_lowercase();
    let _language = parts.next()?;
    let encoded = parts.next()?;
    let bytes = percent_decode(encoded)?;

    match charset.as_str() {
        "utf-8" | "utf8" => String::from_utf8(bytes).ok(),
        "iso-8859-1" | "latin1" => Some(bytes.into_iter().map(char::from).collect()),
        _ => None,
    }
}

fn percent_decode(raw: &str) -> Option<Vec<u8>> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0usize;

    while idx < bytes.len() {
        if bytes[idx] == b'%' {
            let hi = *bytes.get(idx + 1)?;
            let lo = *bytes.get(idx + 2)?;
            let hi = from_hex_digit(hi)?;
            let lo = from_hex_digit(lo)?;
            out.push((hi << 4) | lo);
            idx += 3;
            continue;
        }

        out.push(bytes[idx]);
        idx += 1;
    }

    Some(out)
}

fn from_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn extension_from_url(url: &Url) -> Option<String> {
    let ext = Path::new(url.path()).extension()?.to_str()?;
    let mut cleaned = String::with_capacity(ext.len());
    for ch in ext.chars() {
        if ch.is_ascii_alphanumeric() {
            cleaned.push(ch.to_ascii_lowercase());
        }
    }

    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn redacted_url_for_display(url: &Url) -> String {
    let mut redacted = url.clone();
    if redacted.query().is_some() {
        redacted.set_query(Some("<redacted>"));
    }
    redacted.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    use reqwest::header::{HeaderMap, HeaderValue, CONTENT_DISPOSITION};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn unique_id() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    /// Every file left in an execution's scratch space.
    fn scratch_files(ctx: &ExecutionContext) -> Vec<PathBuf> {
        fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, files);
                } else {
                    files.push(path);
                }
            }
        }
        let mut files = Vec::new();
        if let Some(root) = ctx.scratch.root() {
            walk(root, &mut files);
        }
        files
    }

    enum ServerResponse {
        Success(Vec<u8>),
        Status {
            code: u16,
            reason: &'static str,
            body: Vec<u8>,
        },
        TruncatedBody {
            announced_len: usize,
            sent: Vec<u8>,
        },
    }

    fn spawn_single_response_server(response: ServerResponse) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            consume_request_headers(&mut stream);

            match response {
                ServerResponse::Success(body) => {
                    write_response(&mut stream, 200, "OK", body.len(), &body);
                }
                ServerResponse::Status { code, reason, body } => {
                    write_response(&mut stream, code, reason, body.len(), &body);
                }
                ServerResponse::TruncatedBody {
                    announced_len,
                    sent,
                } => {
                    write_response(&mut stream, 200, "OK", announced_len, &sent);
                }
            }

            let _ = stream.flush();
        });

        (format!("http://{addr}"), handle)
    }

    fn spawn_sequence_server(
        responses: Vec<ServerResponse>,
    ) -> (String, Arc<AtomicUsize>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let request_count = Arc::new(AtomicUsize::new(0));
        let request_count_for_thread = Arc::clone(&request_count);

        let handle = thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                request_count_for_thread.fetch_add(1, Ordering::SeqCst);
                consume_request_headers(&mut stream);

                match response {
                    ServerResponse::Success(body) => {
                        write_response(&mut stream, 200, "OK", body.len(), &body);
                    }
                    ServerResponse::Status { code, reason, body } => {
                        write_response(&mut stream, code, reason, body.len(), &body);
                    }
                    ServerResponse::TruncatedBody {
                        announced_len,
                        sent,
                    } => {
                        write_response(&mut stream, 200, "OK", announced_len, &sent);
                    }
                }

                let _ = stream.flush();
            }
        });

        (format!("http://{addr}"), request_count, handle)
    }

    fn consume_request_headers(stream: &mut TcpStream) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut buffer = [0u8; 4096];
        let _ = stream.read(&mut buffer);
    }

    fn write_response(
        stream: &mut TcpStream,
        status: u16,
        reason: &str,
        content_len: usize,
        body: &[u8],
    ) {
        let headers = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Length: {content_len}\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(headers.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    }

    #[test]
    fn test_node_contract() {
        let node = DownloaderNode::new();
        assert_eq!(node.node_type(), "downloader");

        let input_ports = node.input_ports();
        assert_eq!(input_ports.len(), 1);
        assert_eq!(input_ports[0].name, "url");
        assert_eq!(input_ports[0].port_type, PortType::Str);
        assert!(input_ports[0].required);

        let output_ports = node.output_ports();
        assert_eq!(output_ports.len(), 1);
        assert_eq!(output_ports[0].name, "path");
        assert_eq!(output_ports[0].port_type, PortType::Path);
        assert!(output_ports[0].required);
        assert!(
            output_ports.iter().all(|port| port.name != "video_path"),
            "legacy output port must not be reintroduced"
        );
    }

    #[test]
    fn test_filename_derivation_prefers_filename_star_over_filename() {
        let url = Url::parse("https://example.com/media/fallback-name.mp4?token=secret").unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_DISPOSITION,
            HeaderValue::from_static(
                "attachment; filename=\"legacy.mp4\"; filename*=UTF-8''preferred%20name.mkv",
            ),
        );

        let (final_path, _) =
            destination_paths_for_url_and_headers(Path::new("/scratch"), &url, Some(&headers));
        let file_name = final_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            file_name, "preferred name.mkv",
            "filename* should take precedence over filename"
        );
    }

    #[test]
    fn test_filename_derivation_sanitizes_traversal_and_unsafe_characters() {
        let url = Url::parse("https://example.com/download?id=1").unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment; filename=\"../../unsafe/..\\episode?.mkv\""),
        );

        let (final_path, _) =
            destination_paths_for_url_and_headers(Path::new("/scratch"), &url, Some(&headers));
        let file_name = final_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            final_path.parent().unwrap(),
            Path::new("/scratch"),
            "sanitized filename must stay in the download directory"
        );
        assert_eq!(
            file_name, "episode.mkv",
            "sanitized filename should keep safe leaf name and extension"
        );
        assert!(!file_name.contains(".."), "must strip traversal segments");
        assert!(!file_name.contains('?'), "must strip unsafe characters");
    }

    #[test]
    fn test_filename_derivation_fallback_chain_without_headers() {
        let basename_url =
            Url::parse("https://example.com/media/My%20Clip.mp4?token=secret").unwrap();
        let (basename_path, _) =
            destination_paths_for_url_and_headers(Path::new("/scratch"), &basename_url, None);
        let basename_file = basename_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();

        assert_eq!(
            basename_file, "My Clip.mp4",
            "when headers are absent, URL basename should be used first"
        );

        let fallback_url = Url::parse("https://example.com/?token=secret").unwrap();
        let (fallback_path, _) =
            destination_paths_for_url_and_headers(Path::new("/scratch"), &fallback_url, None);
        let fallback_file = fallback_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let digest = format!("{:x}", Sha256::digest(fallback_url.as_str().as_bytes()));

        assert_eq!(
            fallback_file,
            format!("video-{digest}"),
            "deterministic fallback should be used when header and URL basename are unavailable"
        );
    }

    #[test]
    fn test_execute_success_downloads_and_returns_path() {
        let id = unique_id();
        let payload = b"fake-video-payload".to_vec();
        let (base_url, server_handle) =
            spawn_single_response_server(ServerResponse::Success(payload.clone()));
        let url = format!("{base_url}/videos/{id}.mp4?token=secret-value");

        let ctx = ExecutionContext::default();
        let mut node = DownloaderNode::new();
        let mut inputs = HashMap::new();
        inputs.insert("url".to_string(), PortData::Str(url.clone()));

        let outputs = node
            .execute(&inputs, &ctx)
            .expect("download should succeed");
        server_handle.join().unwrap();

        let output_path = match outputs.get("path") {
            Some(PortData::Path(path)) => path.clone(),
            _ => panic!("expected path Path output"),
        };

        assert!(output_path.starts_with(downloads_root()));
        assert_eq!(
            output_path.file_name().and_then(|name| name.to_str()),
            Some(format!("{id}.mp4").as_str())
        );
        assert!(output_path.exists(), "downloaded file should exist");

        let file_bytes = fs::read(&output_path).expect("downloaded file should be readable");
        assert_eq!(file_bytes, payload);

        let tmp_path = part_path(&output_path);
        assert!(
            !tmp_path.exists(),
            ".part file should be removed after success"
        );
    }

    #[test]
    fn test_execute_rejects_invalid_scheme() {
        let id = unique_id();
        let url = format!("ftp://example.com/video/{id}.mp4?token=super-secret-token");

        let ctx = ExecutionContext::default();
        let mut node = DownloaderNode::new();
        let mut inputs = HashMap::new();
        inputs.insert("url".to_string(), PortData::Str(url));

        let err = node
            .execute(&inputs, &ctx)
            .err()
            .expect("invalid scheme should fail");
        let msg = err.to_string();

        assert!(
            msg.contains("unsupported downloader URL scheme"),
            "error: {msg}"
        );
        assert!(msg.contains("redacted"), "error should redact query: {msg}");
        assert!(
            !msg.contains("super-secret-token"),
            "error must not leak token: {msg}"
        );
        assert!(
            !msg.contains("failed to start download"),
            "invalid scheme should fail before any network call: {msg}"
        );
    }

    #[test]
    fn test_execute_http_non_success_fails_without_leaking_query_or_leaving_temp_file() {
        let id = unique_id();
        let (base_url, server_handle) = spawn_single_response_server(ServerResponse::Status {
            code: 404,
            reason: "Not Found",
            body: b"missing".to_vec(),
        });
        let url = format!("{base_url}/missing/{id}.mp4?api_key=abc123");

        let ctx = ExecutionContext::default();
        let mut node = DownloaderNode::new();
        let mut inputs = HashMap::new();
        inputs.insert("url".to_string(), PortData::Str(url.clone()));

        let err = node
            .execute(&inputs, &ctx)
            .err()
            .expect("404 response should fail");
        server_handle.join().unwrap();

        let msg = err.to_string();
        assert!(
            msg.contains("HTTP 404"),
            "error should include status code: {msg}"
        );
        assert!(
            !msg.contains("abc123"),
            "error must not leak query value: {msg}"
        );

        assert_eq!(
            scratch_files(&ctx),
            Vec::<PathBuf>::new(),
            "neither the final nor the .part file may remain (on HTTP failure)"
        );
    }

    #[test]
    fn test_execute_cleans_part_file_when_body_read_fails() {
        let id = unique_id();
        let sent = b"tiny".to_vec();
        let announced_len = sent.len() + 32;
        let mut responses = Vec::with_capacity(DOWNLOAD_MAX_ATTEMPTS);
        for _ in 0..DOWNLOAD_MAX_ATTEMPTS {
            responses.push(ServerResponse::TruncatedBody {
                announced_len,
                sent: sent.clone(),
            });
        }
        let (base_url, request_count, server_handle) = spawn_sequence_server(responses);
        let url = format!("{base_url}/broken/{id}.mkv?token=top-secret");

        let ctx = ExecutionContext::default();
        let mut node = DownloaderNode::new();
        let mut inputs = HashMap::new();
        inputs.insert("url".to_string(), PortData::Str(url.clone()));

        let err = node
            .execute(&inputs, &ctx)
            .err()
            .expect("truncated body should fail");
        server_handle.join().unwrap();

        let msg = err.to_string();
        assert!(
            msg.contains("failed while reading HTTP body"),
            "error: {msg}"
        );
        assert!(
            !msg.contains("top-secret"),
            "error must not leak token: {msg}"
        );
        assert_eq!(request_count.load(Ordering::SeqCst), DOWNLOAD_MAX_ATTEMPTS);

        assert_eq!(
            scratch_files(&ctx),
            Vec::<PathBuf>::new(),
            "neither the final nor the .part file may remain (after read failure)"
        );
    }

    #[test]
    fn test_execute_retries_retryable_status_then_succeeds() {
        let id = unique_id();
        let payload = b"retry-success-payload".to_vec();
        let (base_url, request_count, server_handle) = spawn_sequence_server(vec![
            ServerResponse::Status {
                code: 503,
                reason: "Service Unavailable",
                body: b"busy".to_vec(),
            },
            ServerResponse::Success(payload.clone()),
        ]);
        let url = format!("{base_url}/retry/{id}.mp4?token=retry-secret");

        let ctx = ExecutionContext::default();
        let mut node = DownloaderNode::new();
        let mut inputs = HashMap::new();
        inputs.insert("url".to_string(), PortData::Str(url.clone()));

        let outputs = node
            .execute(&inputs, &ctx)
            .expect("retryable status should eventually succeed");
        server_handle.join().unwrap();

        assert_eq!(request_count.load(Ordering::SeqCst), 2);

        let output_path = match outputs.get("path") {
            Some(PortData::Path(path)) => path.clone(),
            _ => panic!("expected path Path output"),
        };
        assert_eq!(fs::read(&output_path).unwrap(), payload);

        let tmp_path = part_path(&output_path);
        assert!(
            !tmp_path.exists(),
            ".part file should be removed after retries"
        );
    }

    #[test]
    fn test_execute_retry_exhaustion_redacts_query_and_cleans_temp_file() {
        let id = unique_id();
        let (base_url, request_count, server_handle) = spawn_sequence_server(vec![
            ServerResponse::Status {
                code: 503,
                reason: "Service Unavailable",
                body: b"busy-1".to_vec(),
            },
            ServerResponse::Status {
                code: 503,
                reason: "Service Unavailable",
                body: b"busy-2".to_vec(),
            },
            ServerResponse::Status {
                code: 503,
                reason: "Service Unavailable",
                body: b"busy-3".to_vec(),
            },
        ]);
        let url = format!("{base_url}/retry-fail/{id}.mp4?api_key=super-secret-value");

        let ctx = ExecutionContext::default();
        let mut node = DownloaderNode::new();
        let mut inputs = HashMap::new();
        inputs.insert("url".to_string(), PortData::Str(url.clone()));

        let err = node
            .execute(&inputs, &ctx)
            .err()
            .expect("retry exhaustion should fail");
        server_handle.join().unwrap();

        let msg = err.to_string();
        assert!(
            msg.contains("download failed after") && msg.contains("HTTP 503"),
            "error should report retry exhaustion with status: {msg}"
        );
        assert!(msg.contains("redacted"), "error should redact query: {msg}");
        assert!(
            !msg.contains("super-secret-value"),
            "error must not leak token: {msg}"
        );
        assert_eq!(request_count.load(Ordering::SeqCst), DOWNLOAD_MAX_ATTEMPTS);

        assert_eq!(
            scratch_files(&ctx),
            Vec::<PathBuf>::new(),
            "neither the final nor the .part file may remain (after retry exhaustion)"
        );
    }

    fn download(url: &str, ctx: &ExecutionContext) -> PathBuf {
        let mut node = DownloaderNode::new();
        let inputs = HashMap::from([("url".to_string(), PortData::Str(url.to_string()))]);
        match node
            .execute(&inputs, ctx)
            .expect("download should succeed")
            .remove("path")
        {
            Some(PortData::Path(path)) => path,
            _ => panic!("expected path Path output"),
        }
    }

    fn downloads_root() -> PathBuf {
        std::env::temp_dir().join("videnoa").join("downloads")
    }

    fn single_downloader_graph(url: &str) -> crate::graph::PipelineGraph {
        serde_json::from_value(serde_json::json!({
            "nodes": [{ "id": "dl", "node_type": "Downloader", "params": { "url": url } }],
            "connections": []
        }))
        .expect("valid downloader graph")
    }

    fn output_path(outputs: &HashMap<String, HashMap<String, PortData>>) -> PathBuf {
        match outputs.get("dl").and_then(|node| node.get("path")) {
            Some(PortData::Path(path)) => path.clone(),
            _ => panic!("expected dl.path output"),
        }
    }

    #[test]
    fn same_file_name_from_different_urls_does_not_collide() {
        // Given: two URLs that end in the same file name but serve different bodies.
        let name = format!("{}.mkv", unique_id());
        let (first_base, first_server) =
            spawn_single_response_server(ServerResponse::Success(b"first".to_vec()));
        let (second_base, second_server) =
            spawn_single_response_server(ServerResponse::Success(b"second".to_vec()));
        let ctx = ExecutionContext::default();

        // When: one execution downloads both.
        let first = download(&format!("{first_base}/a/{name}"), &ctx);
        let second = download(&format!("{second_base}/b/{name}"), &ctx);
        first_server.join().unwrap();
        second_server.join().unwrap();

        // Then: each keeps its own file, under the original file name.
        assert_ne!(first, second);
        assert_eq!(
            first.file_name().and_then(|n| n.to_str()),
            Some(name.as_str())
        );
        assert_eq!(
            second.file_name().and_then(|n| n.to_str()),
            Some(name.as_str())
        );
        assert_eq!(fs::read(&first).unwrap(), b"first");
        assert_eq!(fs::read(&second).unwrap(), b"second");
    }

    #[test]
    fn concurrent_executions_download_into_separate_directories() {
        // Given: two executions (jobs) fetching the same file name at the same time.
        let name = format!("{}.mp4", unique_id());
        let results = thread::scope(|scope| {
            let workers: Vec<_> = [b"job-one".to_vec(), b"job-two".to_vec()]
                .into_iter()
                .map(|body| {
                    let name = name.clone();
                    scope.spawn(move || {
                        let (base, server) =
                            spawn_single_response_server(ServerResponse::Success(body.clone()));
                        let ctx = ExecutionContext::default();
                        let path = download(&format!("{base}/{name}"), &ctx);
                        server.join().unwrap();
                        (path.clone(), fs::read(&path).unwrap(), body)
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect::<Vec<_>>()
        });

        // Then: neither job sees the other's file or temporary file.
        assert_ne!(results[0].0.parent(), results[1].0.parent());
        for (path, bytes, body) in &results {
            assert_eq!(bytes, body, "{} holds another job's bytes", path.display());
            assert!(path.starts_with(downloads_root()));
        }
    }

    #[test]
    fn downloads_are_removed_when_the_execution_ends() {
        // Given: a file downloaded during an execution.
        let (base, server) =
            spawn_single_response_server(ServerResponse::Success(b"payload".to_vec()));
        let ctx = ExecutionContext::default();
        let path = download(&format!("{base}/{}.mp4", unique_id()), &ctx);
        server.join().unwrap();
        assert!(
            path.exists(),
            "the file must exist while the execution runs"
        );

        // When: the execution ends.
        drop(ctx);

        // Then: its download directory is gone.
        assert!(!path.exists());
        let execution_dir = path
            .ancestors()
            .find(|dir| dir.parent() == Some(downloads_root().as_path()))
            .expect("download lives in a per-execution directory");
        assert!(!execution_dir.exists());
    }

    #[test]
    fn top_level_executor_removes_downloads_when_the_job_returns() {
        // Given: a job whose only node downloads a file.
        let (base, server) =
            spawn_single_response_server(ServerResponse::Success(b"payload".to_vec()));
        let graph = single_downloader_graph(&format!("{base}/{}.mkv", unique_id()));
        let registry = crate::registry::build_default_registry();

        // When: the job runs to completion, as the server and the CLI run it.
        let outputs = crate::executor::SequentialExecutor::execute(&graph, &registry)
            .expect("job should succeed");
        server.join().unwrap();

        // Then: the downloaded file was cleaned up with the job.
        let path = output_path(&outputs);
        assert!(path.starts_with(downloads_root()));
        assert!(!path.exists(), "download should be removed at job end");
    }

    #[test]
    fn nested_executions_keep_downloads_until_the_outer_job_ends() {
        // Given: an outer job context and an inner workflow that downloads.
        let (base, server) =
            spawn_single_response_server(ServerResponse::Success(b"nested".to_vec()));
        let dir = std::env::temp_dir().join(format!("videnoa-nested-dl-{}", unique_id()));
        fs::create_dir_all(&dir).unwrap();
        let inner_path = dir.join("inner.json");
        let inner = serde_json::json!({
            "nodes": [
                { "id": "dl", "node_type": "Downloader",
                  "params": { "url": format!("{base}/{}.mkv", unique_id()) } },
                { "id": "wf_out", "node_type": "WorkflowOutput",
                  "params": { "ports": [{ "name": "path", "port_type": "Path" }] } }
            ],
            "connections": [
                { "from_node": "dl", "from_port": "path", "to_node": "wf_out",
                  "to_port": "path", "port_type": "Path" }
            ]
        });
        fs::write(&inner_path, serde_json::to_string(&inner).unwrap()).unwrap();
        let mut workflow = crate::nodes::workflow_io::WorkflowNode::from_params(&HashMap::from([
            (
                "workflow_path".to_string(),
                serde_json::json!(inner_path.to_string_lossy()),
            ),
            (
                "interface_outputs".to_string(),
                serde_json::json!([{ "name": "path", "port_type": "Path" }]),
            ),
        ]));
        let outer = ExecutionContext::default();

        // When: the inner workflow finishes inside the outer job.
        let outputs = workflow
            .execute(&HashMap::new(), &outer)
            .expect("nested workflow should succeed");
        server.join().unwrap();
        let path = match outputs.get("path") {
            Some(PortData::Path(path)) => path.clone(),
            _ => panic!("expected nested path output"),
        };

        // Then: the file outlives the inner run and goes away with the outer job.
        assert_eq!(fs::read(&path).unwrap(), b"nested");
        drop(outer);
        assert!(!path.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
