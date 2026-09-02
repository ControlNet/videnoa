use std::error::Error;

use serde_json::json;
use videnoa_controller::domain::{
    AttemptId, ComputeSlots, ConcurrencyLimit, LoginRequest, RemoteJobId, SessionId, SseEventId,
    SubmissionKey, TaskId, WorkerApiUrl, WorkerId,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[test]
fn branded_ids_share_wire_shape_but_keep_distinct_rust_types() -> TestResult {
    // Given: one stable UUID spelling crossing each persisted/public identifier boundary.
    let value = json!("00000000-0000-4000-8000-000000000001");

    // When/Then: every brand preserves the UUID wire form without becoming a string alias.
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<TaskId>(value.clone())?)?,
        value
    );
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<AttemptId>(value.clone())?)?,
        value
    );
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<WorkerId>(value.clone())?)?,
        value
    );
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<SessionId>(value.clone())?)?,
        value
    );
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<RemoteJobId>(value.clone())?)?,
        value
    );
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<SubmissionKey>(value.clone())?)?,
        value
    );
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<SseEventId>(value.clone())?)?,
        value
    );
    Ok(())
}

#[test]
fn worker_api_url_normalizes_and_rejects_unsafe_forms() -> TestResult {
    // Given: a valid mixed-case base URL and malformed or policy-invalid alternatives.
    // When: the values cross the typed worker URL boundary.
    let normalized = WorkerApiUrl::parse("HTTPS://Example.COM:443/api//")?;

    // Then: the valid URL has one canonical trailing slash and invalid forms fail typed parsing.
    assert_eq!(
        serde_json::to_value(normalized)?,
        json!("https://example.com/api/")
    );
    for invalid in [
        "not a url",
        "ftp://example.com",
        "https://user:pass@example.com",
        "https://example.com/?token=value",
        "https://example.com/#fragment",
    ] {
        assert!(
            WorkerApiUrl::parse(invalid).is_err(),
            "accepted URL: {invalid}"
        );
    }
    Ok(())
}

#[test]
fn positive_counts_reject_zero_and_overflow() {
    // Given/When/Then: slots and concurrency deserialize only from nonzero u16 values.
    for value in [json!(0), json!(65_536)] {
        assert!(serde_json::from_value::<ComputeSlots>(value.clone()).is_err());
        assert!(serde_json::from_value::<ConcurrencyLimit>(value).is_err());
    }
}

#[test]
fn login_request_debug_and_schema_do_not_expose_passwords() -> TestResult {
    // Given: a deterministic login request used only at the deserialization boundary.
    let request: LoginRequest = serde_json::from_value(json!({"password": "test-only-value"}))?;

    // When: diagnostics format the request and unknown schema fields are parsed.
    let debug = format!("{request:?}");

    // Then: the raw value is absent and the request remains strict.
    assert!(!debug.contains("test-only-value"));
    assert!(debug.contains("[redacted]"));
    assert!(serde_json::from_value::<LoginRequest>(json!({
        "password": "test-only-value",
        "unexpected": true
    }))
    .is_err());
    Ok(())
}
