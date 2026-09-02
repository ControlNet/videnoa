use std::num::NonZeroU32;
use std::time::Duration;

use videnoa_controller::config::RetryConfig;
use videnoa_controller::domain::{FailureCode, FailureInfo, FailureStage};
use videnoa_controller::lifecycle::{
    AutomaticRetry, JitterSample, Lifecycle, LifecycleFailure, RemoteAmbiguityStage, ResumeStage,
    RetryDecision, RetryMode, RetryPolicy,
};

fn retry_config() -> RetryConfig {
    RetryConfig {
        initial: Duration::from_secs(2),
        maximum: Duration::from_secs(10),
        max_attempts: NonZeroU32::new(4).unwrap_or(NonZeroU32::MIN),
    }
}

fn failure(stage: FailureStage, code: FailureCode, retryable: bool) -> FailureInfo {
    FailureInfo {
        failure_stage: stage,
        failure_code: code,
        message: "fixture".to_owned(),
        retryable,
    }
}

#[test]
fn automatic_retry_is_bounded_exponential_with_deterministic_jitter() {
    // Given: a bounded policy and the maximum jitter sample.
    let policy = RetryPolicy::from_config(&retry_config());
    let jitter = JitterSample::try_from(10_000_u16).unwrap_or_default();

    // When: each automatic transient class consumes retry counts.
    let operations = [
        AutomaticRetry::Upload,
        AutomaticRetry::Download,
        AutomaticRetry::Health,
        AutomaticRetry::Cleanup,
    ];

    // Then: every class shares the bounded schedule and stops at the configured count.
    for operation in operations {
        assert_eq!(
            policy.decide(operation, 0, jitter),
            RetryDecision::Schedule {
                operation,
                retry_count: 1,
                delay: Duration::from_secs(2),
            },
        );
        assert_eq!(
            policy.decide(operation, 1, jitter),
            RetryDecision::Schedule {
                operation,
                retry_count: 2,
                delay: Duration::from_secs(4),
            },
        );
        assert_eq!(
            policy.decide(operation, 3, jitter),
            RetryDecision::Schedule {
                operation,
                retry_count: 4,
                delay: Duration::from_secs(10),
            },
        );
        assert_eq!(
            policy.decide(operation, 4, jitter),
            RetryDecision::Exhausted
        );
    }
}

#[test]
fn jitter_sample_rejects_values_outside_the_closed_unit_interval() {
    // Given/When: jitter samples at and beyond the fixed-point boundary are parsed.
    let minimum = JitterSample::try_from(0_u16);
    let maximum = JitterSample::try_from(10_000_u16);
    let overflow = JitterSample::try_from(10_001_u16);

    // Then: only the closed zero-to-one range is representable.
    assert!(minimum.is_ok());
    assert!(maximum.is_ok());
    assert!(overflow.is_err());
}

#[test]
fn failed_processing_creates_a_new_attempt_but_downstream_failures_resume() {
    // Given: retryable processing and downstream failures.
    let cases = [
        (
            failure(
                FailureStage::Processing,
                FailureCode::ProcessingFailed,
                true,
            ),
            RetryMode::NewProcessingAttempt,
        ),
        (
            failure(FailureStage::Download, FailureCode::TransferFailed, true),
            RetryMode::Resume(ResumeStage::Downloading),
        ),
        (
            failure(
                FailureStage::Verification,
                FailureCode::VerificationFailed,
                true,
            ),
            RetryMode::Resume(ResumeStage::Verifying),
        ),
        (
            failure(
                FailureStage::Publication,
                FailureCode::PublicationFailed,
                true,
            ),
            RetryMode::Resume(ResumeStage::Publishing),
        ),
        (
            failure(
                FailureStage::RemoteCleanup,
                FailureCode::CleanupFailed,
                true,
            ),
            RetryMode::Resume(ResumeStage::RemoteCleanup),
        ),
    ];

    // When/Then: processing is the only class that creates a new compute attempt.
    for (failure, expected) in cases {
        assert_eq!(Lifecycle::retry_mode(&failure), expected);
    }
}

#[test]
fn ambiguity_is_non_retryable_even_if_persisted_metadata_claims_otherwise() {
    // Given: contradictory retryable flags on both ambiguity codes.
    let remote = failure(
        FailureStage::Submission,
        FailureCode::RemoteStateAmbiguous,
        true,
    );
    let publication = failure(
        FailureStage::Publication,
        FailureCode::PublicationAmbiguous,
        true,
    );

    // When/Then: code taxonomy outranks mutable persisted retryability metadata.
    assert_eq!(Lifecycle::retry_mode(&remote), RetryMode::Blocked);
    assert_eq!(Lifecycle::retry_mode(&publication), RetryMode::Blocked);
}

#[test]
fn restart_cancelled_remote_job_is_a_retryable_processing_failure() {
    // Given: a remote job cancelled by worker restart.
    let failure = LifecycleFailure::restart_cancelled("worker restarted");

    // When: it is projected into the locked persisted failure contract.
    let info = failure.info();

    // Then: it requires explicit processing retry rather than automatic resubmission.
    assert_eq!(info.failure_stage, FailureStage::Processing);
    assert_eq!(info.failure_code, FailureCode::ProcessingFailed);
    assert!(info.retryable);
    assert_eq!(
        Lifecycle::retry_mode(&info),
        RetryMode::NewProcessingAttempt
    );
}

#[test]
fn typed_ambiguity_failures_cannot_be_marked_retryable() {
    // Given: typed remote and publication ambiguity reports.
    let remote = LifecycleFailure::remote_state_ambiguous(
        RemoteAmbiguityStage::Submission,
        "remote evidence is missing",
    );
    let publication = LifecycleFailure::publication_ambiguous("destination ownership is unknown");

    // When/Then: both projections use stable ambiguity codes and disable retry.
    assert_eq!(
        remote.info().failure_code,
        FailureCode::RemoteStateAmbiguous
    );
    assert!(!remote.info().retryable);
    assert_eq!(
        publication.info().failure_code,
        FailureCode::PublicationAmbiguous
    );
    assert!(!publication.info().retryable);
}
