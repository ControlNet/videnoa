use crate::persistence::WorkerRecord;
use crate::recovery::StagePermit;
use crate::remote::{CacheInvalidation, CompatibilityCatalog, PayloadLimits, VidenoaClient};
use crate::scheduler::RuntimeSettings;

pub(super) struct ProbedWorker {
    pub(super) record: WorkerRecord,
    pub(super) stage: StagePermit,
}

pub(super) enum ProbeOutcome {
    Healthy {
        worker: ProbedWorker,
        catalog: CompatibilityCatalog,
    },
    Failed {
        worker: ProbedWorker,
        failure: ProbeFailure,
    },
}

pub(super) enum ProbeFailure {
    Client,
    Health,
    Capabilities,
    Authentication,
}

impl ProbeFailure {
    pub(super) const fn invalidation(&self) -> CacheInvalidation {
        match self {
            Self::Health => CacheInvalidation::HealthFailure,
            Self::Client | Self::Capabilities | Self::Authentication => {
                CacheInvalidation::RemoteError
            }
        }
    }

    pub(super) const fn message(&self) -> &'static str {
        match self {
            Self::Authentication => "worker authentication failed; check the saved worker password",
            Self::Client => "worker remote client initialization failed",
            Self::Health => "worker health check failed",
            Self::Capabilities => "worker capability refresh failed",
        }
    }
}

pub(super) async fn probe(
    record: WorkerRecord,
    cached: Option<CompatibilityCatalog>,
    settings: RuntimeSettings,
    limits: PayloadLimits,
    stage: StagePermit,
) -> ProbeOutcome {
    let worker = ProbedWorker { record, stage };
    let Ok(client) = VidenoaClient::new_with_password(
        worker.record.api_url.clone(),
        settings.remote_timeouts(),
        limits,
        worker.record.password.as_ref(),
    ) else {
        return ProbeOutcome::Failed {
            worker,
            failure: ProbeFailure::Client,
        };
    };
    match client.health().await {
        Ok(health) if health.is_healthy() => {}
        Err(error) => {
            tracing::warn!(worker_id = %worker.record.id, error = %error, "Worker health request failed");
            return ProbeOutcome::Failed {
                worker,
                failure: ProbeFailure::Health,
            };
        }
        Ok(_) => {
            return ProbeOutcome::Failed {
                worker,
                failure: ProbeFailure::Health,
            };
        }
    }
    let catalog = match cached {
        Some(catalog) => catalog,
        None => match client.capabilities().await {
            Ok(catalog) => catalog,
            Err(error) => {
                tracing::warn!(worker_id = %worker.record.id, error = %error, "Worker capability request failed");
                return ProbeOutcome::Failed {
                    worker,
                    failure: if matches!(
                        error,
                        crate::remote::VidenoaClientError::ClientStatus { status: 401 | 403 }
                    ) {
                        ProbeFailure::Authentication
                    } else {
                        ProbeFailure::Capabilities
                    },
                };
            }
        },
    };
    ProbeOutcome::Healthy { worker, catalog }
}
