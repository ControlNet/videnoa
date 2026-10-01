//! Periodic removal of revoked and expired durable sessions.

use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::time::{interval, MissedTickBehavior};
use tokio_util::sync::CancellationToken;

use super::{AuthError, AuthService};

/// Cadence of the expired-session purge after the startup pass.
pub const SESSION_PURGE_INTERVAL: Duration = Duration::from_hours(1);

impl AuthService {
    /// Deletes revoked, absolutely expired, and idle-expired sessions.
    ///
    /// # Errors
    /// Returns an error when session persistence fails.
    pub async fn purge_expired_sessions(&self, now: DateTime<Utc>) -> Result<u64, AuthError> {
        Ok(self.inner.store.purge_expired_sessions(now).await?)
    }

    /// Purges expired sessions immediately and then once per `cadence` until `shutdown`.
    ///
    /// Persistence failures are logged and retried at the next tick; they never stop the
    /// Controller because lazy revocation keeps expired sessions unusable in the meantime.
    pub async fn run_session_maintenance(self, cadence: Duration, shutdown: CancellationToken) {
        let mut ticks = interval(cadence);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return,
                _ = ticks.tick() => {}
            }
            match self.purge_expired_sessions(Utc::now()).await {
                Ok(purged) => tracing::debug!(purged, "Purged expired sessions"),
                Err(error) => tracing::warn!(%error, "Expired session purge failed"),
            }
        }
    }
}
