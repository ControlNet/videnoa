use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde_json::Value;
use videnoa_controller::domain::{
    AttemptId, InputExtension, InputPath, OutputExtension, OutputPath, SourceReference,
    SubmissionKey, TaskCreateRequest, TaskId, TaskSource, WorkflowName,
};
use videnoa_controller::lifecycle::{
    AdvanceCommand, JitterSample, LifecycleService, ReserveCommand, SubmissionEvidence,
};
use videnoa_controller::persistence::{InputContentIdentity, InputIdentity, NewTask};

use crate::mock_videnoa::server::MockVidenoa;

use super::{Fixture, PreparedTask, TestResult};

impl Fixture {
    pub async fn reserved_task(&self, input_bytes: Vec<u8>) -> TestResult<PreparedTask> {
        let task_id = TaskId::random();
        let input_path = self.input_root.join(format!("{task_id}.mkv"));
        tokio::fs::write(&input_path, &input_bytes).await?;
        let rooted = self.paths.open_input(&input_path)?;
        let attempt_id = AttemptId::random();
        self.store
            .insert_task(&NewTask {
                id: task_id,
                request: TaskCreateRequest {
                    input_path: InputPath::new(path_string(&input_path)?),
                    output_path: OutputPath::new(path_string(
                        &self.output_root.join(format!("{task_id}.mp4")),
                    )?),
                    workflow: WorkflowName::new("eligible-workflow.json"),
                    priority: 10,
                    source: TaskSource::Api,
                    source_reference: Some(SourceReference::new("task-12")),
                },
                input_extension: InputExtension::new("mkv"),
                output_extension: OutputExtension::new("mp4"),
                input_size: rooted.snapshot().length,
                input_mtime: DateTime::<Utc>::from(rooted.snapshot().modified),
                input_identity: InputIdentity::new(rooted.snapshot().platform_identity()),
                input_content_identity: InputContentIdentity::new(
                    rooted.snapshot().content_identity(),
                ),
                created_at: self.now,
            })
            .await?;
        LifecycleService::new(self.store.clone())
            .reserve(&ReserveCommand {
                task_id,
                expected_task_version: 0,
                worker_id: self.worker_id,
                attempt_id,
                submission_key: SubmissionKey::random(),
                reserved_at: self.now,
            })
            .await?;
        Ok(PreparedTask {
            task_id,
            attempt_id,
        })
    }

    pub async fn remote_completed(
        &self,
        server: &MockVidenoa,
        output_bytes: &[u8],
    ) -> TestResult<PreparedTask> {
        let prepared = self.reserved_task(vec![7_u8; 20_000]).await?;
        let executor = self.executor()?;
        let upload = executor
            .upload(prepared.task_id, self.now, JitterSample::try_from(0)?)
            .await?;
        let evidence = match upload {
            videnoa_controller::scheduler::UploadOutcome::Staged(evidence) => evidence,
            other => {
                return Err(std::io::Error::other(format!("unexpected upload: {other:?}")).into())
            }
        };
        let service = LifecycleService::new(self.store.clone());
        self.advance(
            prepared.task_id,
            prepared.attempt_id,
            AdvanceCommand::StartSubmission,
        )
        .await?;
        let attempt = self.attempt(prepared.attempt_id).await?;
        let params = BTreeMap::from([
            (
                "input".to_owned(),
                Value::String(evidence.remote_input_path.as_str().to_owned()),
            ),
            (
                "output".to_owned(),
                Value::String(evidence.remote_output_path.as_str().to_owned()),
            ),
        ]);
        let submitted = self
            .client()?
            .run(
                &WorkflowName::new("eligible-workflow.json"),
                attempt.attempt.submission_key,
                &params,
            )
            .await?;
        self.advance(
            prepared.task_id,
            prepared.attempt_id,
            AdvanceCommand::PersistSubmission(SubmissionEvidence {
                remote_job_id: submitted.receipt.id,
                remote_input_path: evidence.remote_input_path,
                remote_output_path: evidence.remote_output_path,
            }),
        )
        .await?;
        server
            .complete_job(
                &submitted.receipt.id.to_string(),
                &format!("{}/output.mp4", prepared.task_id),
                output_bytes,
            )
            .await?;
        let task = self.task(prepared.task_id).await?;
        let attempt = self.attempt(prepared.attempt_id).await?;
        service
            .advance(&task, &attempt, AdvanceCommand::FinishProcessing, self.now)
            .await?;
        Ok(prepared)
    }

    pub async fn mark_uploading(&self, prepared: &PreparedTask) -> TestResult {
        self.advance(
            prepared.task_id,
            prepared.attempt_id,
            AdvanceCommand::StartUpload,
        )
        .await
    }

    pub async fn mark_downloading(&self, prepared: &PreparedTask) -> TestResult {
        self.advance(
            prepared.task_id,
            prepared.attempt_id,
            AdvanceCommand::StartDownload,
        )
        .await
    }

    async fn advance(
        &self,
        task_id: TaskId,
        attempt_id: AttemptId,
        command: AdvanceCommand,
    ) -> TestResult {
        let task = self.task(task_id).await?;
        let attempt = self.attempt(attempt_id).await?;
        LifecycleService::new(self.store.clone())
            .advance(&task, &attempt, command, self.now)
            .await?;
        Ok(())
    }
}

fn path_string(path: &Path) -> TestResult<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| std::io::Error::other("test path is not UTF-8").into())
}
