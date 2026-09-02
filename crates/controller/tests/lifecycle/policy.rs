use videnoa_controller::domain::TaskStatus;
use videnoa_controller::lifecycle::{
    CancelAction, CommandKind, Lifecycle, LifecycleErrorCode, RecoveryAction, TransitionTarget,
};

const STATES: [TaskStatus; 14] = [
    TaskStatus::Queued,
    TaskStatus::Reserved,
    TaskStatus::Uploading,
    TaskStatus::Staged,
    TaskStatus::Submitting,
    TaskStatus::Processing,
    TaskStatus::RemoteCompleted,
    TaskStatus::Downloading,
    TaskStatus::Verifying,
    TaskStatus::Publishing,
    TaskStatus::RemoteCleanup,
    TaskStatus::Completed,
    TaskStatus::Failed,
    TaskStatus::Cancelled,
];

const COMMANDS: [CommandKind; 15] = [
    CommandKind::Reserve,
    CommandKind::StartUpload,
    CommandKind::FinishUpload,
    CommandKind::StartSubmission,
    CommandKind::PersistSubmission,
    CommandKind::FinishProcessing,
    CommandKind::StartDownload,
    CommandKind::FinishDownload,
    CommandKind::FinishVerification,
    CommandKind::FinishPublication,
    CommandKind::FinishCleanup,
    CommandKind::RequestCancellation,
    CommandKind::FinishCancellation,
    CommandKind::Fail,
    CommandKind::Retry,
];

#[test]
fn legal_command_table_is_exhaustive_for_every_state() {
    // Given: the complete locked lifecycle and command sets.
    let expected = [
        &[
            CommandKind::Reserve,
            CommandKind::RequestCancellation,
            CommandKind::Fail,
        ][..],
        &[
            CommandKind::StartUpload,
            CommandKind::RequestCancellation,
            CommandKind::Fail,
        ],
        &[
            CommandKind::FinishUpload,
            CommandKind::RequestCancellation,
            CommandKind::FinishCancellation,
            CommandKind::Fail,
        ],
        &[
            CommandKind::StartSubmission,
            CommandKind::RequestCancellation,
            CommandKind::FinishCancellation,
            CommandKind::Fail,
        ],
        &[
            CommandKind::PersistSubmission,
            CommandKind::RequestCancellation,
            CommandKind::Fail,
        ],
        &[
            CommandKind::FinishProcessing,
            CommandKind::RequestCancellation,
            CommandKind::FinishCancellation,
            CommandKind::Fail,
        ],
        &[
            CommandKind::StartDownload,
            CommandKind::RequestCancellation,
            CommandKind::FinishCancellation,
            CommandKind::Fail,
        ],
        &[
            CommandKind::FinishDownload,
            CommandKind::RequestCancellation,
            CommandKind::FinishCancellation,
            CommandKind::Fail,
        ],
        &[
            CommandKind::FinishVerification,
            CommandKind::RequestCancellation,
            CommandKind::FinishCancellation,
            CommandKind::Fail,
        ],
        &[CommandKind::FinishPublication, CommandKind::Fail],
        &[CommandKind::FinishCleanup, CommandKind::Fail],
        &[],
        &[CommandKind::Retry],
        &[],
    ];

    // When/Then: each state exposes exactly its table row and no hidden command succeeds.
    for (index, state) in STATES.into_iter().enumerate() {
        assert_eq!(Lifecycle::commands(state), expected[index]);
        for command in COMMANDS {
            assert_eq!(
                Lifecycle::destination(state, command).is_ok(),
                expected[index].contains(&command),
                "state={state:?}, command={command:?}",
            );
        }
    }
}

#[test]
fn normal_transitions_have_one_typed_destination() {
    // Given: every normal durable lifecycle fact.
    let cases = [
        (
            TaskStatus::Queued,
            CommandKind::Reserve,
            TaskStatus::Reserved,
        ),
        (
            TaskStatus::Reserved,
            CommandKind::StartUpload,
            TaskStatus::Uploading,
        ),
        (
            TaskStatus::Uploading,
            CommandKind::FinishUpload,
            TaskStatus::Staged,
        ),
        (
            TaskStatus::Staged,
            CommandKind::StartSubmission,
            TaskStatus::Submitting,
        ),
        (
            TaskStatus::Submitting,
            CommandKind::PersistSubmission,
            TaskStatus::Processing,
        ),
        (
            TaskStatus::Processing,
            CommandKind::FinishProcessing,
            TaskStatus::RemoteCompleted,
        ),
        (
            TaskStatus::RemoteCompleted,
            CommandKind::StartDownload,
            TaskStatus::Downloading,
        ),
        (
            TaskStatus::Downloading,
            CommandKind::FinishDownload,
            TaskStatus::Verifying,
        ),
        (
            TaskStatus::Verifying,
            CommandKind::FinishVerification,
            TaskStatus::Publishing,
        ),
        (
            TaskStatus::Publishing,
            CommandKind::FinishPublication,
            TaskStatus::RemoteCleanup,
        ),
        (
            TaskStatus::RemoteCleanup,
            CommandKind::FinishCleanup,
            TaskStatus::Completed,
        ),
    ];

    // When/Then: every fact resolves to the single locked destination.
    for (state, command, destination) in cases {
        assert_eq!(
            Lifecycle::destination(state, command).ok(),
            Some(TransitionTarget::Status(destination)),
        );
    }
    assert_eq!(
        Lifecycle::destination(TaskStatus::Failed, CommandKind::Retry).ok(),
        Some(TransitionTarget::RetryByFailure),
    );
}

#[test]
fn recovery_classifies_every_state_exactly_once() {
    // Given: the complete lifecycle state set.
    let expected = [
        RecoveryAction::AwaitReservation,
        RecoveryAction::BeginUpload,
        RecoveryAction::ReconcileUpload,
        RecoveryAction::BeginSubmission,
        RecoveryAction::ReconcileSubmission,
        RecoveryAction::PollProcessing,
        RecoveryAction::BeginDownload,
        RecoveryAction::RestartDownload,
        RecoveryAction::Reverify,
        RecoveryAction::ReconcilePublication,
        RecoveryAction::RetryCleanup,
        RecoveryAction::Completed,
        RecoveryAction::Failed,
        RecoveryAction::Cancelled,
    ];

    // When/Then: every state maps to one non-optional recovery classification.
    for (index, state) in STATES.into_iter().enumerate() {
        assert_eq!(Lifecycle::recovery(state), expected[index]);
    }
}

#[test]
fn cancellation_matrix_matches_irreversibility_boundaries() {
    // Given: each state before, during, and after irreversible publication.
    let accepted = [
        (TaskStatus::Queued, CancelAction::CancelLocal),
        (TaskStatus::Reserved, CancelAction::CancelLocal),
        (TaskStatus::Uploading, CancelAction::AbortUploadAndClean),
        (TaskStatus::Staged, CancelAction::CleanStaged),
        (TaskStatus::Submitting, CancelAction::ReconcileSubmission),
        (TaskStatus::Processing, CancelAction::CancelRemoteAndClean),
        (
            TaskStatus::RemoteCompleted,
            CancelAction::AbortDownstreamAndClean,
        ),
        (
            TaskStatus::Downloading,
            CancelAction::AbortDownstreamAndClean,
        ),
        (TaskStatus::Verifying, CancelAction::AbortDownstreamAndClean),
    ];
    let rejected = [
        TaskStatus::Publishing,
        TaskStatus::RemoteCleanup,
        TaskStatus::Completed,
        TaskStatus::Failed,
        TaskStatus::Cancelled,
    ];

    // When/Then: cancellable states return their exact action and late states return conflict.
    for (state, action) in accepted {
        assert_eq!(Lifecycle::cancellation(state).ok(), Some(action));
    }
    for state in rejected {
        let error = Lifecycle::cancellation(state).expect_err("late cancellation must fail");
        assert_eq!(error.code(), LifecycleErrorCode::Conflict);
    }
}
