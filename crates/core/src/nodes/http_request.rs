use std::collections::HashMap;
use std::io::Read;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};
use reqwest::Method;
use url::Url;

use crate::node::{ExecutionContext, Node, PortDefinition};
use crate::types::{PortData, PortType};

pub struct HttpRequestNode;

const DEFAULT_TIMEOUT_MS: i64 = 30_000;
const MIN_TIMEOUT_MS: i64 = 100;
const MAX_TIMEOUT_MS: i64 = 300_000;

const DEFAULT_MAX_RETRIES: i64 = 2;
const MIN_MAX_RETRIES: i64 = 0;
const MAX_MAX_RETRIES: i64 = 5;

const DEFAULT_RETRY_BACKOFF_MS: i64 = 250;
const MIN_RETRY_BACKOFF_MS: i64 = 0;
const MAX_RETRY_BACKOFF_MS: i64 = 10_000;

const DEFAULT_MAX_RESPONSE_BYTES: i64 = 1_048_576;
const MIN_MAX_RESPONSE_BYTES: i64 = 1;
const MAX_MAX_RESPONSE_BYTES: i64 = 16_777_216;

const CONNECT_TIMEOUT_CAP_MS: i64 = 15_000;

impl HttpRequestNode {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HttpRequestNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for HttpRequestNode {
    fn node_type(&self) -> &str {
        "HttpRequest"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![
            PortDefinition {
                name: "method".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("GET")),
            },
            PortDefinition {
                name: "url".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "headers_json".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("{}")),
            },
            PortDefinition {
                name: "body".to_string(),
                port_type: PortType::Str,
                required: false,
                default_value: Some(serde_json::json!("")),
            },
            PortDefinition {
                name: "timeout_ms".to_string(),
                port_type: PortType::Int,
                required: false,
                default_value: Some(serde_json::json!(DEFAULT_TIMEOUT_MS)),
            },
            PortDefinition {
                name: "max_retries".to_string(),
                port_type: PortType::Int,
                required: false,
                default_value: Some(serde_json::json!(DEFAULT_MAX_RETRIES)),
            },
            PortDefinition {
                name: "retry_backoff_ms".to_string(),
                port_type: PortType::Int,
                required: false,
                default_value: Some(serde_json::json!(DEFAULT_RETRY_BACKOFF_MS)),
            },
            PortDefinition {
                name: "max_response_bytes".to_string(),
                port_type: PortType::Int,
                required: false,
                default_value: Some(serde_json::json!(DEFAULT_MAX_RESPONSE_BYTES)),
            },
        ]
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![
            PortDefinition {
                name: "status_code".to_string(),
                port_type: PortType::Int,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "ok".to_string(),
                port_type: PortType::Bool,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "response_body".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "response_url".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            },
            PortDefinition {
                name: "content_type".to_string(),
                port_type: PortType::Str,
                required: true,
                default_value: None,
            },
        ]
    }

    fn execute(
        &mut self,
        inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        let method = parse_method(inputs)?;
        let raw_url = parse_required_str(inputs, "url")?;
        let url = parse_http_url(raw_url)?;
        let redacted_url = redacted_url_for_display(&url);

        let headers_json = parse_optional_str(inputs, "headers_json", "{}");
        let headers_json_context = sanitize_headers_json_for_context(headers_json.as_str());
        let headers = parse_headers_json(headers_json.as_str()).with_context(|| {
            format!(
                "HttpRequest invalid headers_json for {}: {}",
                redacted_url, headers_json_context
            )
        })?;

        let body = parse_optional_str(inputs, "body", "");
        let timeout_ms = parse_clamped_i64(
            inputs,
            "timeout_ms",
            DEFAULT_TIMEOUT_MS,
            MIN_TIMEOUT_MS,
            MAX_TIMEOUT_MS,
        );
        let max_retries = parse_clamped_i64(
            inputs,
            "max_retries",
            DEFAULT_MAX_RETRIES,
            MIN_MAX_RETRIES,
            MAX_MAX_RETRIES,
        );
        let retry_backoff_ms = parse_clamped_i64(
            inputs,
            "retry_backoff_ms",
            DEFAULT_RETRY_BACKOFF_MS,
            MIN_RETRY_BACKOFF_MS,
            MAX_RETRY_BACKOFF_MS,
        );
        let max_response_bytes = parse_clamped_i64(
            inputs,
            "max_response_bytes",
            DEFAULT_MAX_RESPONSE_BYTES,
            MIN_MAX_RESPONSE_BYTES,
            MAX_MAX_RESPONSE_BYTES,
        ) as usize;

        let request_timeout = Duration::from_millis(timeout_ms as u64);
        let connect_timeout = Duration::from_millis(timeout_ms.min(CONNECT_TIMEOUT_CAP_MS) as u64);

        let client = reqwest::blocking::Client::builder()
            .connect_timeout(connect_timeout)
            .timeout(request_timeout)
            .build()
            .context("failed to build HTTP client for HttpRequest")?;

        let max_attempts = (max_retries as usize).saturating_add(1);
        let request_context = sanitized_context(format!(
            "method={} url={} headers_json={} body={}",
            method.as_str(),
            redacted_url,
            headers_json_context,
            body
        ));

        for attempt in 1..=max_attempts {
            match execute_once(
                &client,
                method.clone(),
                &url,
                headers.clone(),
                body.clone(),
                max_response_bytes,
                &request_context,
            ) {
                Ok(outputs) => return Ok(outputs),
                Err(attempt_error) => {
                    if attempt_error.retryable && attempt < max_attempts {
                        let delay_ms = (retry_backoff_ms as u64).saturating_mul(attempt as u64);
                        std::thread::sleep(Duration::from_millis(delay_ms));
                        continue;
                    }

                    if attempt_error.retryable {
                        return Err(anyhow!(
                            "HttpRequest failed after {} attempts ({}): {}",
                            max_attempts,
                            request_context,
                            attempt_error.error
                        ));
                    }

                    return Err(attempt_error.error);
                }
            }
        }

        Err(anyhow!(
            "HttpRequest failed after {} attempts ({})",
            max_attempts,
            request_context
        ))
    }
}

fn execute_once(
    client: &reqwest::blocking::Client,
    method: Method,
    url: &Url,
    headers: HeaderMap,
    body: String,
    max_response_bytes: usize,
    request_context: &str,
) -> std::result::Result<HashMap<String, PortData>, RequestAttemptError> {
    let mut request = client.request(method, url.as_str());
    if !headers.is_empty() {
        request = request.headers(headers);
    }

    if !body.is_empty() {
        request = request.body(body);
    }

    let mut response = request.send().map_err(|err| {
        let wrapped = anyhow!(
            "HttpRequest transport error for {}: {}",
            request_context,
            sanitized_context(err.to_string())
        );
        if is_retryable_reqwest_error(&err) {
            RequestAttemptError::retryable(wrapped)
        } else {
            RequestAttemptError::fatal(wrapped)
        }
    })?;

    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let response_url = response.url().to_string();

    let response_body =
        read_response_body_limited(&mut response, max_response_bytes).map_err(|err| {
            let wrapped = anyhow!(
                "HttpRequest failed reading body for {}: {}",
                request_context,
                sanitized_context(err.to_string())
            );
            RequestAttemptError::fatal(wrapped)
        })?;

    let outputs = HashMap::from([
        (
            "status_code".to_string(),
            PortData::Int(status.as_u16() as i64),
        ),
        ("ok".to_string(), PortData::Bool(status.is_success())),
        ("response_body".to_string(), PortData::Str(response_body)),
        ("response_url".to_string(), PortData::Str(response_url)),
        ("content_type".to_string(), PortData::Str(content_type)),
    ]);

    Ok(outputs)
}

fn parse_method(inputs: &HashMap<String, PortData>) -> Result<Method> {
    let method_raw = parse_optional_str(inputs, "method", "GET");
    Method::from_bytes(method_raw.trim().to_ascii_uppercase().as_bytes())
        .with_context(|| sanitized_context(format!("HttpRequest invalid method: {}", method_raw)))
}

fn parse_required_str<'a>(inputs: &'a HashMap<String, PortData>, key: &str) -> Result<&'a str> {
    match inputs.get(key) {
        Some(PortData::Str(value)) => Ok(value.as_str()),
        Some(_) => bail!("HttpRequest input '{key}' must be Str"),
        None => bail!("HttpRequest input '{key}' is required"),
    }
}

fn parse_optional_str(inputs: &HashMap<String, PortData>, key: &str, default: &str) -> String {
    match inputs.get(key) {
        Some(PortData::Str(value)) => value.clone(),
        _ => default.to_string(),
    }
}

fn parse_clamped_i64(
    inputs: &HashMap<String, PortData>,
    key: &str,
    default: i64,
    min: i64,
    max: i64,
) -> i64 {
    match inputs.get(key) {
        Some(PortData::Int(value)) => (*value).clamp(min, max),
        _ => default.clamp(min, max),
    }
}

fn parse_http_url(raw: &str) -> Result<Url> {
    let parsed = Url::parse(raw)
        .with_context(|| format!("invalid HttpRequest URL: {}", sanitized_context(raw)))?;

    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        scheme => {
            let redacted = redacted_url_for_display(&parsed);
            bail!(
                "unsupported HttpRequest URL scheme '{scheme}' for '{redacted}' (expected http/https)"
            )
        }
    }
}

fn parse_headers_json(raw: &str) -> Result<HeaderMap> {
    let parsed: serde_json::Value = serde_json::from_str(raw)
        .with_context(|| sanitized_context(format!("headers_json is not valid JSON: {raw}")))?;

    let object = parsed
        .as_object()
        .ok_or_else(|| anyhow!("headers_json must be a JSON object"))?;

    let mut headers = HeaderMap::new();
    for (key, value) in object {
        let value_text = value
            .as_str()
            .ok_or_else(|| anyhow!("header '{key}' must have string value"))?;

        let name = HeaderName::from_bytes(key.as_bytes())
            .with_context(|| format!("invalid header name '{}': not RFC-compliant", key))?;
        let val = HeaderValue::from_str(value_text)
            .with_context(|| format!("invalid header value for '{}': not RFC-compliant", key))?;
        headers.insert(name, val);
    }

    Ok(headers)
}

fn read_response_body_limited(
    response: &mut reqwest::blocking::Response,
    max_response_bytes: usize,
) -> Result<String> {
    let mut bytes = Vec::with_capacity(max_response_bytes.min(16 * 1024));
    let mut buffer = [0u8; 8192];

    loop {
        let read_count = response
            .read(&mut buffer)
            .context("failed to read HTTP response body")?;
        if read_count == 0 {
            break;
        }

        if bytes.len().saturating_add(read_count) > max_response_bytes {
            bail!("response body exceeded max_response_bytes={max_response_bytes}");
        }

        bytes.extend_from_slice(&buffer[..read_count]);
    }

    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn is_retryable_reqwest_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request() || err.is_body()
}

fn redacted_url_for_display(url: &Url) -> String {
    if url.query().is_none() {
        return url.to_string();
    }

    let mut no_query = url.clone();
    no_query.set_query(None);
    format!("{}?<redacted>", no_query)
}

fn sanitized_context(text: impl AsRef<str>) -> String {
    crate::logging::redact_sensitive_text(text.as_ref())
}

fn sanitize_headers_json_for_context(raw: &str) -> String {
    let parsed = match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(value) => value,
        Err(_) => return "<unparseable>".to_string(),
    };

    let object = match parsed.as_object() {
        Some(object) => object,
        None => return "<invalid-headers-json>".to_string(),
    };

    let mut redacted = serde_json::Map::new();
    for (key, value) in object {
        let redacted_value = match value {
            serde_json::Value::String(_) => serde_json::Value::String("***REDACTED***".to_string()),
            _ => serde_json::Value::String("***REDACTED***".to_string()),
        };
        redacted.insert(key.clone(), redacted_value);
    }

    serde_json::Value::Object(redacted).to_string()
}

struct RequestAttemptError {
    retryable: bool,
    error: anyhow::Error,
}

impl RequestAttemptError {
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    fn spawn_single_response_server(raw_response: String) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("local addr");

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept test client");
            consume_request_headers(&mut stream);
            stream
                .write_all(raw_response.as_bytes())
                .expect("write response");
            let _ = stream.flush();
        });

        (format!("http://{addr}"), handle)
    }

    fn consume_request_headers(stream: &mut TcpStream) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut buffer = [0u8; 4096];
        let _ = stream.read(&mut buffer);
    }

    fn run_node_with_inputs(
        inputs: HashMap<String, PortData>,
    ) -> Result<HashMap<String, PortData>> {
        let mut node = HttpRequestNode::new();
        node.execute(&inputs, &ExecutionContext::default())
    }

    fn expect_int(outputs: &HashMap<String, PortData>, key: &str) -> i64 {
        match outputs.get(key) {
            Some(PortData::Int(value)) => *value,
            _ => panic!("expected Int output for key '{key}'"),
        }
    }

    fn expect_bool(outputs: &HashMap<String, PortData>, key: &str) -> bool {
        match outputs.get(key) {
            Some(PortData::Bool(value)) => *value,
            _ => panic!("expected Bool output for key '{key}'"),
        }
    }

    fn expect_str(outputs: &HashMap<String, PortData>, key: &str) -> String {
        match outputs.get(key) {
            Some(PortData::Str(value)) => value.clone(),
            _ => panic!("expected Str output for key '{key}'"),
        }
    }

    #[test]
    fn test_http_request_contract() {
        let node = HttpRequestNode::new();
        assert_eq!(node.node_type(), "HttpRequest");

        let input_ports = node.input_ports();
        assert_eq!(input_ports.len(), 8);
        assert_eq!(input_ports[0].name, "method");
        assert_eq!(input_ports[0].port_type, PortType::Str);
        assert_eq!(input_ports[1].name, "url");
        assert_eq!(input_ports[1].port_type, PortType::Str);

        let output_ports = node.output_ports();
        assert_eq!(output_ports.len(), 5);
        assert_eq!(output_ports[0].name, "status_code");
        assert_eq!(output_ports[0].port_type, PortType::Int);
        assert_eq!(output_ports[1].name, "ok");
        assert_eq!(output_ports[1].port_type, PortType::Bool);
        assert_eq!(output_ports[2].name, "response_body");
        assert_eq!(output_ports[2].port_type, PortType::Str);
        assert_eq!(output_ports[3].name, "response_url");
        assert_eq!(output_ports[3].port_type, PortType::Str);
        assert_eq!(output_ports[4].name, "content_type");
        assert_eq!(output_ports[4].port_type, PortType::Str);
    }

    #[test]
    fn test_http_request_200_response_fields() {
        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK".to_string();
        let (base_url, server_handle) = spawn_single_response_server(response);

        let inputs = HashMap::from([
            ("method".to_string(), PortData::Str("GET".to_string())),
            (
                "url".to_string(),
                PortData::Str(format!("{base_url}/health?token=abc")),
            ),
        ]);

        let outputs = run_node_with_inputs(inputs).expect("request should succeed");
        server_handle.join().expect("server thread join");

        assert_eq!(expect_int(&outputs, "status_code"), 200);
        assert!(expect_bool(&outputs, "ok"));
        assert_eq!(expect_str(&outputs, "response_body"), "OK");
        assert_eq!(
            expect_str(&outputs, "content_type"),
            "text/plain; charset=utf-8"
        );
        assert!(expect_str(&outputs, "response_url").starts_with(&base_url));
    }

    #[test]
    fn test_http_request_non_2xx_returns_ok_false() {
        let response = "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: 7\r\nConnection: close\r\n\r\nmissing".to_string();
        let (base_url, server_handle) = spawn_single_response_server(response);

        let inputs = HashMap::from([
            ("method".to_string(), PortData::Str("GET".to_string())),
            (
                "url".to_string(),
                PortData::Str(format!("{base_url}/missing")),
            ),
        ]);

        let outputs = run_node_with_inputs(inputs).expect("non-2xx should still return outputs");
        server_handle.join().expect("server thread join");

        assert_eq!(expect_int(&outputs, "status_code"), 404);
        assert!(!expect_bool(&outputs, "ok"));
        assert_eq!(expect_str(&outputs, "response_body"), "missing");
        assert_eq!(expect_str(&outputs, "content_type"), "text/plain");
    }

    #[test]
    fn test_http_request_rejects_invalid_scheme() {
        let inputs = HashMap::from([
            ("method".to_string(), PortData::Str("GET".to_string())),
            (
                "url".to_string(),
                PortData::Str("ftp://example.com/resource?token=secret-token".to_string()),
            ),
        ]);

        let err = run_node_with_inputs(inputs)
            .err()
            .expect("invalid scheme should fail");
        let msg = err.to_string();

        assert!(
            msg.contains("unsupported HttpRequest URL scheme"),
            "error: {msg}"
        );
        assert!(
            msg.contains("<redacted>"),
            "error should redact URL query: {msg}"
        );
        assert!(
            !msg.contains("secret-token"),
            "error must not leak token: {msg}"
        );
    }

    #[test]
    fn test_http_request_error_messages_redact_secrets() {
        let inputs = HashMap::from([
            ("method".to_string(), PortData::Str("GET".to_string())),
            (
                "url".to_string(),
                PortData::Str("http://127.0.0.1:1/fail?token=my-token".to_string()),
            ),
            (
                "headers_json".to_string(),
                PortData::Str(
                    r#"{"Authorization":"Bearer my-auth-token","api_key":"my-key"}"#.to_string(),
                ),
            ),
            (
                "body".to_string(),
                PortData::Str("password=my-password".to_string()),
            ),
            ("timeout_ms".to_string(), PortData::Int(500)),
            ("max_retries".to_string(), PortData::Int(0)),
        ]);

        let err = run_node_with_inputs(inputs)
            .err()
            .expect("connection failure should error");
        let msg = err.to_string();

        assert!(
            msg.contains("***REDACTED***"),
            "error should contain redaction markers: {msg}"
        );
        assert!(!msg.contains("my-token"), "must redact query token: {msg}");
        assert!(
            !msg.contains("my-auth-token"),
            "must redact auth header: {msg}"
        );
        assert!(!msg.contains("my-key"), "must redact api_key value: {msg}");
        assert!(
            !msg.contains("my-password"),
            "must redact body password: {msg}"
        );
    }

    #[test]
    fn test_http_request_missing_url_fails_fast() {
        let inputs = HashMap::from([("method".to_string(), PortData::Str("GET".to_string()))]);

        let err = run_node_with_inputs(inputs)
            .err()
            .expect("missing url input should fail");
        assert_eq!(err.to_string(), "HttpRequest input 'url' is required");
    }

    #[test]
    fn test_http_request_url_type_mismatch_fails_fast() {
        let inputs = HashMap::from([
            ("method".to_string(), PortData::Str("GET".to_string())),
            ("url".to_string(), PortData::Int(123)),
        ]);

        let err = run_node_with_inputs(inputs)
            .err()
            .expect("non-string url input should fail");
        assert_eq!(err.to_string(), "HttpRequest input 'url' must be Str");
    }

    #[test]
    fn test_http_request_invalid_headers_json_redacts_raw_secret_payload() {
        let inputs = HashMap::from([
            ("method".to_string(), PortData::Str("GET".to_string())),
            (
                "url".to_string(),
                PortData::Str("https://example.com/api?token=secret-token".to_string()),
            ),
            (
                "headers_json".to_string(),
                PortData::Str(r#"{"Authorization":"Bearer super-secret""#.to_string()),
            ),
        ]);

        let err = run_node_with_inputs(inputs)
            .err()
            .expect("invalid headers_json should fail");
        let msg = err.to_string();

        assert!(
            msg.contains("HttpRequest invalid headers_json for"),
            "error: {msg}"
        );
        assert!(msg.contains("<unparseable>"), "error: {msg}");
        assert!(
            msg.contains("<redacted>"),
            "error should redact query: {msg}"
        );
        assert!(
            !msg.contains("super-secret") && !msg.contains("secret-token"),
            "error must not leak raw secret values: {msg}"
        );
    }

    #[test]
    fn test_http_request_response_size_limit_fails_with_explicit_message() {
        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 8\r\nConnection: close\r\n\r\nTOOLONG!".to_string();
        let (base_url, server_handle) = spawn_single_response_server(response);

        let inputs = HashMap::from([
            ("method".to_string(), PortData::Str("GET".to_string())),
            (
                "url".to_string(),
                PortData::Str(format!("{base_url}/payload?api_key=size-secret")),
            ),
            ("max_response_bytes".to_string(), PortData::Int(4)),
            ("max_retries".to_string(), PortData::Int(0)),
        ]);

        let err = run_node_with_inputs(inputs)
            .err()
            .expect("oversized response must fail deterministically");
        server_handle.join().expect("server thread join");
        let msg = err.to_string();

        assert!(msg.contains("failed reading body"), "error: {msg}");
        assert!(
            msg.contains("response body exceeded max_response_bytes=4"),
            "error: {msg}"
        );
        assert!(
            !msg.contains("size-secret") && msg.contains("<redacted>"),
            "error should redact URL secrets: {msg}"
        );
    }
}
