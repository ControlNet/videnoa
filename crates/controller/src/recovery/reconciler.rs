use std::num::NonZeroU16;
use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::domain::{FailureCode, FailureStage, TaskStatus};
use crate::lifecycle::{AdvanceCommand, Lifecycle, LifecycleFailure, LifecycleService};
use crate::persistence::{AttemptRecord, Store, SubmissionOwner, TaskRecord};
use crate::remote::VidenoaClient;

use super::{
    local_first_command, RecoveryCommandKind, RecoveryConfig, RecoveryError, RecoveryReport,
    ShutdownCoordinator, StagePermit,
};

mod startup;

const RECOVERY_PAGE_SIZE: NonZeroU16 = match NonZeroU16::new(256) {
    Some(value) => value,
    None => NonZeroU16::MIN,
};

#[derive(Clone)]
pub struct Reconciler {
    pub(super) store: Store,
    pub(super) config: RecoveryConfig,
    pub(super) shutdown: ShutdownCoordinator,
    pub(super) checkpoint_observer: Arc<dyn crate::scheduler::TransferCheckpointObserver>,
    pub(super) submission_owner: SubmissionOwner,
    recovery_page_size: NonZeroU16,
}

impl Reconciler {
    #[must_use]
    pub fn new(store: Store, config: RecoveryConfig, shutdown: ShutdownCoordinator) -> Self {
        Self {
            store,
            config,
            shutdown,
            checkpoint_observer: crate::scheduler::noop_observer(),
            submission_owner: SubmissionOwner::random(),
            recovery_page_size: RECOVERY_PAGE_SIZE,
        }
    }

    #[must_use]
    pub fn with_recovery_page_size(mut self, page_size: NonZeroU16) -> Self {
        self.recovery_page_size = page_size;
        self
    }

    #[must_use]
    pub fn with_checkpoint_observer(
        mut self,
        observer: Arc<dyn crate::scheduler::TransferCheckpointObserver>,
    ) -> Self {
        self.checkpoint_observer = observer;
        self
    }

    pub(super) async fn checkpoint(&self, point: crate::scheduler::TransferCheckpointPoint) {
        self.checkpoint_observer.checkpoint(point).await;
    }

    /// Reconciles one durable task after an in-process stage advances.
    ///
    /// # Errors
    /// Returns a typed error when the task disappears or reconciliation cannot commit.
    pub async fn reconcile_task_id(
        &self,
        task_id: crate::domain::TaskId,
        now: DateTime<Utc>,
    ) -> Result<RecoveryReport, RecoveryError> {
        let task = self
            .store
            .task(task_id)
            .await?
            .ok_or(RecoveryError::Conflict)?;
        let mut report = RecoveryReport::default();
        let Some(stage) = self.shutdown.begin_stage() else {
            report.defer(task_id);
            return Ok(report);
        };
        self.reconcile_task(task, now, &stage, &mut report).await?;
        Ok(report)
    }

    async fn reconcile_task(
        &self,
        task: TaskRecord,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        if task.status == TaskStatus::Queued {
            report.push(task.id, RecoveryCommandKind::AwaitReservation);
            return Ok(());
        }
        let Some(attempt) = self.store.current_attempt(task.id).await? else {
            return self
                .fail_ambiguous(
                    &task,
                    None,
                    "durable task is missing its current attempt",
                    now,
                    stage,
                    report,
                )
                .await;
        };
        if task.cancel_requested_at.is_none() {
            if let Some(command) = local_first_command(Lifecycle::recovery(task.status)) {
                report.push(task.id, command);
                return Ok(());
            }
        }
        let Some(worker_id) = task.worker_id else {
            return self
                .fail_ambiguous(
                    &task,
                    Some(&attempt),
                    "durable task is missing its assigned worker",
                    now,
                    stage,
                    report,
                )
                .await;
        };
        let Some(worker) = self.store.worker(worker_id).await? else {
            return self
                .fail_ambiguous(
                    &task,
                    Some(&attempt),
                    "assigned worker record is missing",
                    now,
                    stage,
                    report,
                )
                .await;
        };
        let client = VidenoaClient::new(
            worker.api_url.clone(),
            self.config.remote_timeouts(),
            self.config.limits,
        )?;
        match client.health().await {
            Ok(_) => {}
            Err(error) if error.is_transient() => {
                self.defer_worker(&worker, task.id, now, stage, report)
                    .await?;
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        }
        if task.cancel_requested_at.is_some() {
            return self
                .reconcile_cancellation(task, attempt, &client, now, stage, report)
                .await;
        }
        self.dispatch(task, attempt, &client, now, stage, report)
            .await
    }

    async fn dispatch(
        &self,
        task: TaskRecord,
        attempt: AttemptRecord,
        client: &VidenoaClient,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        match Lifecycle::recovery(task.status) {
            crate::lifecycle::RecoveryAction::AwaitReservation => {
                report.push(task.id, RecoveryCommandKind::AwaitReservation);
            }
            crate::lifecycle::RecoveryAction::BeginUpload
            | crate::lifecycle::RecoveryAction::ReconcileUpload => {
                report.push(task.id, RecoveryCommandKind::Upload);
            }
            crate::lifecycle::RecoveryAction::BeginSubmission
            | crate::lifecycle::RecoveryAction::ReconcileSubmission => {
                self.reconcile_submission(task, attempt, client, now, stage, report)
                    .await?;
            }
            crate::lifecycle::RecoveryAction::PollProcessing => {
                self.reconcile_processing(task, attempt, client, now, stage, report)
                    .await?;
            }
            crate::lifecycle::RecoveryAction::BeginDownload
            | crate::lifecycle::RecoveryAction::RestartDownload => {
                if task.status == TaskStatus::RemoteCompleted {
                    let _write = stage.begin_write();
                    LifecycleService::new(self.store.clone())
                        .advance(&task, &attempt, AdvanceCommand::StartDownload, now)
                        .await?;
                }
                report.push(task.id, RecoveryCommandKind::Download);
            }
            crate::lifecycle::RecoveryAction::Reverify => {
                report.push(task.id, RecoveryCommandKind::Verify);
            }
            crate::lifecycle::RecoveryAction::ReconcilePublication => {
                report.push(task.id, RecoveryCommandKind::Publish);
            }
            crate::lifecycle::RecoveryAction::RetryCleanup => {
                report.push(task.id, RecoveryCommandKind::Cleanup);
            }
            crate::lifecycle::RecoveryAction::Completed
            | crate::lifecycle::RecoveryAction::Failed
            | crate::lifecycle::RecoveryAction::Cancelled => {
                report.push(task.id, RecoveryCommandKind::Terminal);
            }
        }
        Ok(())
    }

    pub(super) async fn fail_ambiguous(
        &self,
        task: &TaskRecord,
        attempt: Option<&AttemptRecord>,
        message: &str,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        let _write = stage.begin_write();
        LifecycleService::new(self.store.clone())
            .fail_recovery(
                task,
                attempt,
                LifecycleFailure::terminal(
                    task.status,
                    failure_stage(task.status),
                    FailureCode::RemoteStateAmbiguous,
                    message,
                ),
                now,
            )
            .await?;
        report.push(task.id, RecoveryCommandKind::Terminal);
        Ok(())
    }
}

const fn failure_stage(status: TaskStatus) -> FailureStage {
    match status {
        TaskStatus::Queued | TaskStatus::Reserved => FailureStage::Reservation,
        TaskStatus::Uploading => FailureStage::Upload,
        TaskStatus::Staged | TaskStatus::Submitting => FailureStage::Submission,
        TaskStatus::Processing | TaskStatus::RemoteCompleted => FailureStage::Processing,
        TaskStatus::Downloading => FailureStage::Download,
        TaskStatus::Verifying => FailureStage::Verification,
        TaskStatus::Publishing => FailureStage::Publication,
        TaskStatus::RemoteCleanup
        | TaskStatus::Completed
        | TaskStatus::Failed
        | TaskStatus::Cancelled => FailureStage::RemoteCleanup,
    }
}
