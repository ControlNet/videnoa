use chrono::{DateTime, Utc};

use crate::domain::TaskProgress;
use crate::persistence::{AttemptRecord, CasOutcome, TaskRecord};
use crate::remote::{Job, JobProgress, JobStatus};

use super::{Reconciler, RecoveryError, StagePermit};

impl Reconciler {
    pub(super) async fn refresh_processing_progress(
        &self,
        task: &mut TaskRecord,
        attempt: &mut AttemptRecord,
        job: &Job,
        now: DateTime<Utc>,
        stage: &StagePermit,
    ) -> Result<(), RecoveryError> {
        let progress = normalize(job.progress.as_ref(), job.status, &task.progress);
        if progress == task.progress && progress == attempt.attempt.progress {
            return Ok(());
        }
        let _write = stage.begin_write();
        match self
            .store
            .record_processing_progress(task, attempt, &progress, now)
            .await?
        {
            CasOutcome::Applied { new_version } => {
                task.version = new_version;
                task.progress = progress.clone();
                task.updated_at = now;
                attempt.version += 1;
                attempt.attempt.progress = progress;
                attempt.updated_at = now;
                Ok(())
            }
            CasOutcome::Conflict => Err(RecoveryError::Conflict),
        }
    }
}

// Frame ratios are display values; saturating casts are bounded/filtered first.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn normalize(
    sample: Option<&JobProgress>,
    status: JobStatus,
    previous: &TaskProgress,
) -> TaskProgress {
    let mut progress = previous.clone();
    if let Some(sample) = sample {
        progress.processed_frames = Some(sample.current_frame);
        progress.total_frames = sample.total_frames;
        progress.percent = sample
            .total_frames
            .filter(|total| *total > 0)
            .map_or(0.0, |total| {
                (100.0 * sample.current_frame as f64 / total as f64).clamp(0.0, 100.0) as f32
            });
        progress.frames_per_second =
            (sample.fps.is_finite() && sample.fps > 0.0).then_some(sample.fps);
        progress.eta_seconds = sample
            .eta_seconds
            .filter(|eta| eta.is_finite() && *eta >= 0.0)
            .map(|eta| eta.ceil() as u64);
        progress.bytes_transferred = None;
        progress.bytes_total = None;
    }
    if status == JobStatus::Completed {
        progress.percent = 100.0;
        progress.eta_seconds = Some(0);
    }
    progress
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_totals_and_invalid_rates_remain_finite_and_optional() {
        let empty = TaskProgress {
            percent: 0.0,
            processed_frames: None,
            total_frames: None,
            frames_per_second: None,
            eta_seconds: None,
            bytes_transferred: None,
            bytes_total: None,
        };
        for total in [None, Some(0)] {
            let sample = JobProgress {
                current_frame: u64::MAX,
                total_frames: total,
                fps: f32::NAN,
                eta_seconds: Some(f64::INFINITY),
            };
            let progress = normalize(Some(&sample), JobStatus::Running, &empty);
            assert!(progress.percent.abs() < f32::EPSILON);
            assert_eq!(progress.frames_per_second, None);
            assert_eq!(progress.eta_seconds, None);
            assert!(serde_json::to_string(&progress).is_ok());
        }
        let sample = JobProgress {
            current_frame: 20,
            total_frames: Some(10),
            fps: -1.0,
            eta_seconds: Some(-2.0),
        };
        let progress = normalize(Some(&sample), JobStatus::Running, &empty);
        assert!((progress.percent - 100.0).abs() < f32::EPSILON);
        assert_eq!(progress.eta_seconds, None);
    }
}
