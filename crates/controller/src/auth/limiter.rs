use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};

const FAILURE_LIMIT: usize = 5;
const WINDOW: Duration = Duration::minutes(5);
// Bounds limiter memory when many distinct peers fail inside one window.
const MAX_TRACKED_PEERS: usize = 10_000;

type Attempts = VecDeque<DateTime<Utc>>;

#[derive(Clone, Default)]
pub struct LoginLimiter {
    failures: Arc<Mutex<HashMap<IpAddr, Attempts>>>,
}

impl LoginLimiter {
    /// Reports whether the peer has exhausted its failure budget inside the window.
    ///
    /// The check is consulted before password verification so a limited peer
    /// never consumes a verification permit; limited attempts are not recorded.
    pub fn is_limited(&self, address: IpAddr, now: DateTime<Utc>) -> bool {
        let mut failures = self.lock();
        let Some(attempts) = failures.get_mut(&address) else {
            return false;
        };
        expire(attempts, now);
        if attempts.is_empty() {
            failures.remove(&address);
            return false;
        }
        attempts.len() >= FAILURE_LIMIT
    }

    /// Records a failed password check and reports whether the budget is now exceeded.
    pub fn record_failure(&self, address: IpAddr, now: DateTime<Utc>) -> bool {
        let mut failures = self.lock();
        failures.retain(|peer, attempts| {
            if *peer == address {
                return true;
            }
            expire(attempts, now);
            !attempts.is_empty()
        });
        let attempts = failures.entry(address).or_default();
        expire(attempts, now);
        attempts.push_back(now);
        let exceeded = attempts.len() > FAILURE_LIMIT;
        if failures.len() > MAX_TRACKED_PEERS {
            evict_oldest(&mut failures, address);
        }
        exceeded
    }

    pub fn clear(&self, address: IpAddr) {
        self.lock().remove(&address);
    }

    #[cfg(test)]
    fn tracked_peers(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<IpAddr, Attempts>> {
        self.failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn expire(attempts: &mut Attempts, now: DateTime<Utc>) {
    while attempts
        .front()
        .is_some_and(|attempt| *attempt <= now - WINDOW)
    {
        attempts.pop_front();
    }
}

// Evicts the peer whose latest failure is oldest, never the peer just recorded.
fn evict_oldest(failures: &mut HashMap<IpAddr, Attempts>, keep: IpAddr) {
    let oldest = failures
        .iter()
        .filter(|(peer, _)| **peer != keep)
        .min_by_key(|(_, attempts)| attempts.back().copied())
        .map(|(peer, _)| *peer);
    if let Some(peer) = oldest {
        failures.remove(&peer);
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    fn peer(index: u32) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from(0x0a00_0000 + index))
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000 + seconds, 0).expect("valid timestamp")
    }

    #[test]
    fn budget_is_exhausted_after_the_fifth_failure_and_frees_when_the_window_passes() {
        let limiter = LoginLimiter::default();
        for attempt in 0..FAILURE_LIMIT {
            let now = at(i64::try_from(attempt).expect("small index"));
            assert!(!limiter.is_limited(peer(1), now));
            assert!(!limiter.record_failure(peer(1), now));
        }
        assert!(limiter.is_limited(peer(1), at(10)));
        assert!(!limiter.is_limited(peer(2), at(10)));
        // The oldest failure ages out exactly one window after it was recorded.
        assert!(limiter.is_limited(peer(1), at(299)));
        assert!(!limiter.is_limited(peer(1), at(300)));
    }

    #[test]
    fn expired_peers_are_pruned_on_the_next_recorded_failure() {
        let limiter = LoginLimiter::default();
        limiter.record_failure(peer(1), at(0));
        limiter.record_failure(peer(2), at(60));
        assert_eq!(limiter.tracked_peers(), 2);

        limiter.record_failure(peer(3), at(301));
        assert_eq!(limiter.tracked_peers(), 2);
        assert!(!limiter.is_limited(peer(1), at(301)));

        limiter.record_failure(peer(4), at(400));
        assert_eq!(limiter.tracked_peers(), 2);
    }

    #[test]
    fn tracked_peers_are_capped_by_evicting_the_stalest_entry() {
        let limiter = LoginLimiter::default();
        // Peer 0 is the only entry whose latest failure is the oldest.
        for index in 0..MAX_TRACKED_PEERS {
            let seconds = if index == 0 {
                0
            } else {
                1 + i64::try_from(index % 100).expect("small")
            };
            limiter.record_failure(
                peer(u32::try_from(index).expect("small index")),
                at(seconds),
            );
        }
        assert_eq!(limiter.tracked_peers(), MAX_TRACKED_PEERS);

        let newest = peer(u32::try_from(MAX_TRACKED_PEERS).expect("small index"));
        limiter.record_failure(newest, at(100));
        assert_eq!(limiter.tracked_peers(), MAX_TRACKED_PEERS);
        assert_eq!(limiter.lock().get(&newest).map(VecDeque::len), Some(1));
        assert!(!limiter.lock().contains_key(&peer(0)));
    }
}
