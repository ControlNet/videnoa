//! Upper bounds for harness waits on something that must eventually happen.
//!
//! These bounds only expire when a test is already failing, so they are
//! generous: on slow disks a single fsync was measured stalling for 11 s, and a
//! passing run must not fail because of it. Windows that assert something does
//! *not* happen, polling intervals and sleeps that advance time keep their
//! fixed durations.

use std::time::Duration;

/// Multiplies every eventual-wait bound. Set a larger integer for very slow
/// machines, for example `VIDENOA_TEST_DEADLINE_SCALE=20`.
pub const SCALE_ENV: &str = "VIDENOA_TEST_DEADLINE_SCALE";

const DEFAULT_SCALE: u32 = 6;

/// Returns `base` scaled for an eventual wait.
pub fn eventually(base: Duration) -> Duration {
    base.saturating_mul(scale_from(std::env::var(SCALE_ENV).ok().as_deref()))
}

fn scale_from(value: Option<&str>) -> u32 {
    value
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|scale| *scale > 0)
        .unwrap_or(DEFAULT_SCALE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scale_tolerates_multi_second_fsync_stalls() {
        assert_eq!(scale_from(None), DEFAULT_SCALE);
        assert!(Duration::from_secs(5).saturating_mul(DEFAULT_SCALE) >= Duration::from_secs(30));
    }

    #[test]
    fn scale_override_accepts_only_positive_integers() {
        assert_eq!(scale_from(Some("20")), 20);
        assert_eq!(scale_from(Some(" 3 ")), 3);
        assert_eq!(scale_from(Some("0")), DEFAULT_SCALE);
        assert_eq!(scale_from(Some("-1")), DEFAULT_SCALE);
        assert_eq!(scale_from(Some("fast")), DEFAULT_SCALE);
    }
}
