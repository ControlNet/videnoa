use std::collections::{HashMap, HashSet};

use chrono::Utc;
use tokio::sync::broadcast;
use tokio::task::JoinSet;
use tokio::time::{sleep_until, Instant};

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
        let mut next_scan = Instant::now() + self.poll_interval;

        loop {
            let deadline = next_wakeup(next_scan, not_before.values().copied());
            tokio::select! {
                () = cancellation.cancelled() => break,
                received = self.wakeups.recv() => {
                    if matches!(received, Err(broadcast::error::RecvError::Closed)) {
                        break;
                    }
                    self.fill(&mut stages, &mut active, &not_before).await?;
                }
                () = sleep_until(deadline) => {
                    let now = Instant::now();
                    not_before.retain(|_, deadline| *deadline > now);
                    next_scan = now + self.poll_interval;
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

fn next_wakeup(next_scan: Instant, deadlines: impl Iterator<Item = Instant>) -> Instant {
    deadlines
        .min()
        .map_or(next_scan, |deadline| deadline.min(next_scan))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_loop::DEFAULT_POLL_INTERVAL;

    #[tokio::test(start_paused = true)]
    async fn completed_poll_wakes_at_one_second_deadline_without_extra_scan_delay() {
        let start = Instant::now();
        let first_scan = start + DEFAULT_POLL_INTERVAL;
        // A response arriving just after the scan must not wait for the next two ticks.
        tokio::time::advance(std::time::Duration::from_millis(100)).await;
        let due = Instant::now() + DEFAULT_POLL_INTERVAL;
        assert_eq!(due - start, std::time::Duration::from_millis(1100));
        sleep_until(next_wakeup(first_scan, [due].into_iter())).await;
        assert_eq!(Instant::now(), first_scan);
        let second_scan = Instant::now() + DEFAULT_POLL_INTERVAL;
        sleep_until(next_wakeup(second_scan, [due].into_iter())).await;
        assert_eq!(Instant::now(), due);
    }

    #[test]
    fn idle_scan_and_earliest_task_deadline_are_bounded() {
        let now = Instant::now();
        let scan = now + DEFAULT_POLL_INTERVAL;
        assert_eq!(next_wakeup(scan, std::iter::empty()), scan);
        let early = now + std::time::Duration::from_millis(200);
        assert_eq!(next_wakeup(scan, [scan, early, now].into_iter()), now);
        assert_eq!(next_wakeup(scan, [scan, early].into_iter()), early);
    }
}
