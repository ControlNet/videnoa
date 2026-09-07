use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::domain::{WorkerApiUrl, WorkflowName};

use super::{Compatibility, CompatibilityCatalog};

pub trait MonotonicClock: Clone {
    fn now(&self) -> Duration;
}

#[derive(Clone, Debug)]
pub struct SystemClock {
    started: Instant,
}

impl SystemClock {
    #[must_use]
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl MonotonicClock for SystemClock {
    fn now(&self) -> Duration {
        self.started.elapsed()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheInvalidation {
    HealthFailure,
    Restart,
    RemoteError,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CompatibilityEvidence {
    pub durable: Option<Compatibility>,
    pub live: Option<Compatibility>,
}

#[derive(Clone, Debug)]
struct CacheEntry {
    expires_at: Duration,
    catalog: CompatibilityCatalog,
}

#[derive(Clone, Debug)]
pub struct CapabilityCache<C> {
    clock: C,
    ttl: Duration,
    entries: BTreeMap<String, CacheEntry>,
}

impl<C: MonotonicClock> CapabilityCache<C> {
    #[must_use]
    pub fn new(clock: C, ttl: Duration) -> Self {
        Self {
            clock,
            ttl,
            entries: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, worker: &WorkerApiUrl, catalog: CompatibilityCatalog) {
        let expires_at = self.clock.now().saturating_add(self.ttl);
        self.entries.insert(
            worker.as_url().as_str().to_owned(),
            CacheEntry {
                expires_at,
                catalog,
            },
        );
    }

    #[must_use]
    pub fn catalog(&mut self, worker: &WorkerApiUrl) -> Option<CompatibilityCatalog> {
        self.cached(worker).cloned()
    }

    pub fn invalidate(&mut self, worker: &WorkerApiUrl, reason: CacheInvalidation) {
        match reason {
            CacheInvalidation::HealthFailure
            | CacheInvalidation::Restart
            | CacheInvalidation::RemoteError => {}
        }
        self.entries.remove(worker.as_url().as_str());
    }

    pub fn resolve(
        &mut self,
        worker: &WorkerApiUrl,
        workflow: &WorkflowName,
        evidence: CompatibilityEvidence,
    ) -> Option<Compatibility> {
        evidence.live.or(evidence.durable).or_else(|| {
            self.cached(worker)
                .and_then(|catalog| catalog.compatibility(workflow))
        })
    }

    fn cached(&mut self, worker: &WorkerApiUrl) -> Option<&CompatibilityCatalog> {
        let key = worker.as_url().as_str();
        let expired = self
            .entries
            .get(key)
            .is_some_and(|entry| self.clock.now() >= entry.expires_at);
        if expired {
            self.entries.remove(key);
        }
        self.entries.get(key).map(|entry| &entry.catalog)
    }
}
