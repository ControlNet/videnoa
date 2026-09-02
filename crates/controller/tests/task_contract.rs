use std::error::Error;

use serde_json::json;
use videnoa_controller::domain::{PageRequest, TaskCreateRequest, TaskListQuery};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[test]
fn task_create_preserves_exact_paths_extensions_and_source() -> TestResult {
    // Given: caller-owned paths with independent extensions and opaque punctuation.
    let input = json!({
        "input_path": "/nas/input/Season ../episode.v1.mkv",
        "output_path": "/nas/output/Season ../episode.final.mp4",
        "workflow": "anime upscale ../v2",
        "priority": 17,
        "source": "api",
        "source_reference": "ani-rss:item/0042"
    });

    // When: the shared manual/API request crosses the JSON boundary.
    let request: TaskCreateRequest = serde_json::from_value(input.clone())?;
    let output = serde_json::to_value(request)?;

    // Then: no path, extension, workflow, priority, or source value is rewritten.
    assert_eq!(output, input);
    Ok(())
}

#[test]
fn task_create_rejects_unknown_schema_fields() {
    // Given/When/Then: an otherwise valid request cannot smuggle an unknown field.
    assert!(serde_json::from_value::<TaskCreateRequest>(json!({
        "input_path": "/nas/input/source.mkv",
        "output_path": "/nas/output/result.mp4",
        "workflow": "anime-upscale",
        "priority": 0,
        "source": "manual",
        "source_reference": null,
        "overwrite": true
    }))
    .is_err());
}

#[test]
fn page_request_enforces_locked_bounds() -> TestResult {
    // Given: omitted, boundary, and invalid page values.
    // When/Then: defaults and valid bounds parse while zero, overflow, and negatives fail.
    assert_eq!(PageRequest::try_new(None, 0)?.limit().get(), 100);
    assert_eq!(PageRequest::try_new(Some(500), 9)?.offset().get(), 9);
    assert!(PageRequest::try_new(Some(0), 0).is_err());
    assert!(PageRequest::try_new(Some(501), 0).is_err());
    assert!(PageRequest::try_new(None, -1).is_err());
    assert!(serde_json::from_value::<PageRequest>(
        json!({"limit": 18_446_744_073_709_551_615_u64, "offset": 0})
    )
    .is_err());
    Ok(())
}

#[test]
fn task_list_query_defaults_to_deterministic_priority_order() -> TestResult {
    // Given: an empty task-list query.
    // When: defaults are applied at the typed boundary.
    let query: TaskListQuery = serde_json::from_value(json!({}))?;

    // Then: the page is bounded and ordering is deterministic.
    assert_eq!(query.page.limit().get(), 100);
    assert_eq!(serde_json::to_value(query.sort)?, json!("priority"));
    assert_eq!(serde_json::to_value(query.direction)?, json!("desc"));
    Ok(())
}
