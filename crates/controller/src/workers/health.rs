use std::collections::VecDeque;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::broadcast;
use tokio::task::JoinSet;
use tokio::time::{interval, MissedTickBehavior};

use crate::domain::WorkerCapabilities;
use crate::operations::EventHub;
use crate::persistence::{Store, WorkerHealthUpdate, WorkerRecord};
use crate::recovery::ShutdownCoordinator;
use crate::remote::{CapabilityCache, PayloadLimits, SystemClock};
use crate::scheduler::RuntimeSettings;

use super::{WorkerRegistry, WorkerRegistryError, WorkerRegistryErrorCode};
use probe::{probe, ProbeOutcome};

mod probe;

const MAX_CONCURRENT_PROBES: usize = 8;

#[derive(Debug, thiserror::Error)]
pub enum WorkerHealthError {
    #[error(transparent)]
    Persistence(#[from] crate::persistence::PersistenceError),
    #[error("worker health probe task terminated unexpectedly")]
    ProbeJoin(#[from] tokio::task::JoinError),
    #[error("worker health deadline cannot be represented")]
    DeadlineRange,
    #[error("worker health persistence failed")]
    Registry(#[from] WorkerRegistryError),
}

pub struct WorkerHealthService {
    store: Store,
    registry: WorkerRegistry,
    runtime_settings: RuntimeSettings,
    payload_limits: PayloadLimits,
    shutdown: ShutdownCoordinator,
    wakeups: broadcast::Receiver<()>,
}

impl WorkerHealthService {
    #[must_use]
    pub fn new(
        store: Store,
        runtime_settings: RuntimeSettings,
        payload_limits: PayloadLimits,
        shutdown: ShutdownCoordinator,
        events: &EventHub,
    ) -> Self {
        Self {
            registry: WorkerRegistry::new(store.clone()),
            store,
            runtime_settings,
            payload_limits,
            shutdown,
            wakeups: events.subscribe_wakeups(),
        }
    }

    /// Refreshes enabled workers until coordinated shutdown closes stage intake.
    ///
    /// # Errors
    /// Returns when worker persistence fails or a probe task terminates unexpectedly.
    pub async fn run(mut self) -> Result<(), WorkerHealthError> {
        let cancellation = self.shutdown.cancellation_token();
        let cadence = self.health_cadence();
        let mut cache = CapabilityCache::new(SystemClock::new(), cadence);
        let mut poll = interval(cadence);
        poll.set_missed_tick_behavior(MissedTickBehavior::Delay);
        poll.tick().await;

        loop {
            self.refresh_due(&mut cache).await?;
            tokio::select! {
                () = cancellation.cancelled() => return Ok(()),
                received = self.wakeups.recv() => {
                    if matches!(received, Err(broadcast::error::RecvError::Closed)) {
                        return Ok(());
                    }
                }
                _ = poll.tick() => {}
            }
        }
    }

    async fn refresh_due(
        &self,
        cache: &mut CapabilityCache<SystemClock>,
    ) -> Result<(), WorkerHealthError> {
        let now = Utc::now();
        let mut pending: VecDeque<_> = self
            .store
            .workers()
            .await?
            .into_iter()
            .filter(|worker| due(worker, now))
            .collect();
        let mut probes = JoinSet::new();
        while !pending.is_empty() || !probes.is_empty() {
            while probes.len() < MAX_CONCURRENT_PROBES {
                let Some(worker) = pending.pop_front() else {
                    break;
                };
                let Some(stage) = self.shutdown.begin_stage() else {
                    pending.clear();
                    break;
                };
                let cached = cache.catalog(&worker.api_url);
                let settings = self.runtime_settings.clone();
                let limits = self.payload_limits;
                probes.spawn(probe(worker, cached, settings, limits, stage));
            }
            let Some(joined) = probes.join_next().await else {
                break;
            };
            let outcome = joined?;
            self.persist(outcome, cache, Utc::now()).await?;
        }
        Ok(())
    }

    async fn persist(
        &self,
        outcome: ProbeOutcome,
        cache: &mut CapabilityCache<SystemClock>,
        now: DateTime<Utc>,
    ) -> Result<(), WorkerHealthError> {
        let (update, stage, catalog) = match outcome {
            ProbeOutcome::Healthy { worker, catalog } => (
                WorkerHealthUpdate {
                    id: worker.record.id,
                    expected_version: worker.record.version,
                    online: true,
                    capabilities: WorkerCapabilities {
                        workflows: catalog.eligible_workflows(),
                        refreshed_at: Some(now),
                    },
                    last_seen_at: Some(now),
                    health_retry_count: 0,
                    next_health_check_at: Some(deadline(now, self.health_cadence())?),
                    last_error: None,
                    updated_at: now,
                },
                worker.stage,
                Some(catalog),
            ),
            ProbeOutcome::Failed { worker, failure } => {
                cache.invalidate(&worker.record.api_url, failure.invalidation());
                let (retry_count, delay) = retry(&worker.record, &self.runtime_settings);
                (
                    WorkerHealthUpdate {
                        id: worker.record.id,
                        expected_version: worker.record.version,
                        online: false,
                        capabilities: worker.record.capabilities,
                        last_seen_at: worker.record.last_seen_at,
                        health_retry_count: retry_count,
                        next_health_check_at: Some(deadline(now, delay)?),
                        last_error: Some(failure.message().to_owned()),
                        updated_at: now,
                    },
                    worker.stage,
                    None,
                )
            }
        };
        let _write = stage.begin_write();
        match self.registry.refresh_health(update).await {
            Ok(record) => {
                if let Some(catalog) = catalog {
                    cache.insert(&record.api_url, catalog);
                }
                Ok(())
            }
            Err(error)
                if matches!(
                    error.code(),
                    WorkerRegistryErrorCode::Conflict | WorkerRegistryErrorCode::NotFound
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    fn health_cadence(&self) -> Duration {
        Duration::from_secs(self.runtime_settings.timeout_settings().health_seconds)
    }
}

fn due(worker: &WorkerRecord, now: DateTime<Utc>) -> bool {
    worker.enabled
        && worker
            .next_health_check_at
            .is_none_or(|deadline| deadline <= now)
}

fn retry(worker: &WorkerRecord, settings: &RuntimeSettings) -> (u32, Duration) {
    let retry = settings.retry_settings();
    let retry_count = worker
        .health_retry_count
        .saturating_add(1)
        .min(retry.max_attempts);
    let exponent = worker.health_retry_count.min(63);
    let multiplier = 1_u64.checked_shl(exponent).unwrap_or(u64::MAX);
    let seconds = retry
        .initial_seconds
        .saturating_mul(multiplier)
        .min(retry.maximum_seconds);
    (retry_count, Duration::from_secs(seconds))
}

fn deadline(now: DateTime<Utc>, delay: Duration) -> Result<DateTime<Utc>, WorkerHealthError> {
    now.checked_add_signed(
        chrono::Duration::from_std(delay).map_err(|_| WorkerHealthError::DeadlineRange)?,
    )
    .ok_or(WorkerHealthError::DeadlineRange)
}
