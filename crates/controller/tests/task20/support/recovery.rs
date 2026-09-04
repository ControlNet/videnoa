use videnoa_controller::recovery::{Reconciler, RecoveryConfig, ShutdownCoordinator};
use videnoa_controller::remote::{PayloadLimits, RemoteTimeouts};

use super::{ControllerFixture, TestResult};

impl ControllerFixture {
    pub fn reconciler(&self) -> TestResult<Reconciler> {
        let config = videnoa_controller::config::ControllerConfig::default();
        let paths = videnoa_controller::paths::PathCapabilities::open(&self.path_config)?;
        let limits = PayloadLimits::new(1024 * 1024, 4096)?;
        let timeouts = RemoteTimeouts::new(
            config.timeouts.health,
            config.timeouts.poll,
            config.timeouts.transfer,
        )?;
        Ok(Reconciler::new(
            self.store.clone(),
            RecoveryConfig::new(
                paths,
                timeouts,
                limits,
                config.retry.initial,
                config.retry.maximum,
                config.retry.max_attempts.get(),
            ),
            ShutdownCoordinator::new(),
        ))
    }
}
