use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};

const FAILURE_LIMIT: usize = 5;
const WINDOW: Duration = Duration::minutes(5);

#[derive(Clone, Default)]
pub struct LoginLimiter {
    failures: Arc<Mutex<HashMap<IpAddr, VecDeque<DateTime<Utc>>>>>,
}

impl LoginLimiter {
    pub fn record_failure(&self, address: IpAddr, now: DateTime<Utc>) -> bool {
        let mut failures = self
            .failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let attempts = failures.entry(address).or_default();
        while attempts
            .front()
            .is_some_and(|attempt| *attempt <= now - WINDOW)
        {
            attempts.pop_front();
        }
        attempts.push_back(now);
        attempts.len() > FAILURE_LIMIT
    }

    pub fn clear(&self, address: IpAddr) {
        self.failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&address);
    }
}
