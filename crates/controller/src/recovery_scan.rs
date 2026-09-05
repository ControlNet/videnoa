use std::collections::{HashMap, HashSet};

use chrono::Utc;
use tokio::sync::broadcast;
use tokio::task::JoinSet;
use tokio::time::{interval, Instant, MissedTickBehavior};

use crate::domain::{TaskId, TaskStatus};

use super::{advance_task, OrchestrationError, Orchestrator, StageError, StageOutcome};

impl Orchestrator {
    /// Advances durable task state until shutdown intake closes.
    ///
    /// # Errors
    /// Returns when durable scheduling cannot be scanned or a stage task panics.
    pub async fn run(mut self) -> Result<(), OrchestrationError> {
        let cancellation = self.shutdown.cancellation_token();
        let mut stages = JoinSet::new();
        let mut active = HashSet::new();
        let mut not_before = HashMap::new();
        self.fill(&mut stages, &mut active, &not_before).await?;
        let mut poll = interval(self.poll_interval);
        poll.set_missed_tick_behavior(MissedTickBehavior::Delay);
        poll.tick().await;

        loop {
            tokio::select! {
                () = cancellation.cancelled() => break,
                received = self.wakeups.recv() => {
                    if matches!(received, Err(broadcast::error::RecvError::Closed)) {
                        break;
                    }
                    self.fill(&mut stages, &mut active, &not_before).await?;
                }
                _ = poll.tick() => {
                    let now = Instant::now();
                    not_before.retain(|_, deadline| *deadline > now);
                    self.fill(&mut stages, &mut active, &not_before).await?;
                }
                joined = stages.join_next(), if !stages.is_empty() => {
                    let Some(joined) = joined else {
                        continue;
                    };
                    let outcome = joined?;
                    active.remove(&outcome.task_id);
                    let defer = match outcome.result {
                        Ok(defer) => defer,
                        Err(error) if error.retryable() => true,
                        Err(StageError::Recovery(error)) => return Err(error.into()),
                        Err(StageError::Transfer(error)) => return Err(error.into()),
                    };
                    if defer {
                        not_before.insert(outcome.task_id, Instant::now() + self.poll_interval);
                    } else {
                        self.fill(&mut stages, &mut active, &not_before).await?;
                    }
                }
            }
        }
        stages.abort_all();
        while let Some(joined) = stages.join_next().await {
            if let Err(error) = joined {
                if !error.is_cancelled() {
                    return Err(error.into());
                }
            }
        }
        Ok(())
    }

    async fn fill(
        &self,
        stages: &mut JoinSet<StageOutcome>,
        active: &mut HashSet<TaskId>,
        not_before: &HashMap<TaskId, Instant>,
    ) -> Result<(), OrchestrationError> {
        while self.scheduler.reserve_next(Utc::now()).await?.is_some() {}
        let now = Utc::now();
        let Some(mut scan) = self.store.begin_recovery_scan().await? else {
            return Ok(());
        };
        let mut seen = HashSet::new();
        loop {
            let tasks = self
                .store
                .recovery_tasks(&scan, self.recovery_page_size)
                .await?;
            let Some(last) = tasks.last() else {
                break;
            };
            scan.advance(last);
            for task in tasks {
                if !seen.insert(task.id)
                    || task.status == TaskStatus::Queued
                    || active.contains(&task.id)
                    || not_before.contains_key(&task.id)
                    || self.worker_health_deferred(task.worker_id, now).await?
                {
                    continue;
                }
                let Some(stage) = self.shutdown.begin_stage() else {
                    continue;
                };
                active.insert(task.id);
                let reconciler = self.reconciler.clone();
                let transfers = self.transfers.clone();
                stages.spawn(advance_task(task.id, reconciler, transfers, stage));
            }
        }
        Ok(())
    }

    async fn worker_health_deferred(
        &self,
        worker_id: Option<crate::domain::WorkerId>,
        now: chrono::DateTime<Utc>,
    ) -> Result<bool, crate::persistence::PersistenceError> {
        let Some(worker_id) = worker_id else {
            return Ok(false);
        };
        let Some(worker) = self.store.worker(worker_id).await? else {
            return Ok(false);
        };
        Ok(!worker.online
            && match worker.next_health_check_at {
                Some(deadline) => deadline > now,
                None => worker.health_retry_count > 0,
            })
    }
}
