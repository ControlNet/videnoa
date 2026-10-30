use std::borrow::Cow;

use crate::persistence::WorkerRecord;
use crate::recovery::StagePermit;
use crate::remote::{
    CacheInvalidation, CompatibilityCatalog, PayloadLimits, VidenoaClient, VidenoaClientError,
    PROTOCOL_VERSION,
};
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

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ProbeFailure {
    Client,
    Health,
    Capabilities,
    Authentication,
    Incompatible,
    /// The Worker speaks a protocol version this Controller does not.
    Protocol {
        worker: u32,
    },
    /// The Worker refused the Host name it was addressed by, saying why.
    HostNotAllowed {
        reason: Option<String>,
    },
}

impl ProbeFailure {
    pub(super) const fn invalidation(&self) -> CacheInvalidation {
        match self {
            Self::Health => CacheInvalidation::HealthFailure,
            Self::Client
            | Self::Capabilities
            | Self::Authentication
            | Self::Incompatible
            | Self::Protocol { .. }
            | Self::HostNotAllowed { .. } => CacheInvalidation::RemoteError,
        }
    }

    pub(super) fn message(&self) -> Cow<'static, str> {
        match self {
            Self::Authentication => super::AUTHENTICATION_FAILED.into(),
            Self::Client => "worker remote client initialization failed".into(),
            Self::Health => "worker health check failed".into(),
            Self::Capabilities => "worker capability refresh failed".into(),
            Self::Incompatible => {
                "worker tunnel is incompatible with this Controller; update the worker or the Controller".into()
            }
            Self::Protocol { worker } => format!(
                "worker speaks protocol {worker}, but this Controller supports protocols 1 to \
                 {PROTOCOL_VERSION}; update the Controller"
            )
            .into(),
            Self::HostNotAllowed { reason } => match reason {
                Some(reason) => format!("worker refused this Controller: {reason}").into(),
                None => "worker refused the host name this Controller uses for it".into(),
            },
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
        Ok(health) if health.is_healthy() && !health.protocol_supported() => {
            tracing::warn!(worker_id = %worker.record.id, protocol = health.protocol(), "Worker speaks an unsupported protocol");
            return ProbeOutcome::Failed {
                worker,
                failure: ProbeFailure::Protocol {
                    worker: health.protocol(),
                },
            };
        }
        Ok(health) if health.is_healthy() => {}
        Err(error) => {
            tracing::warn!(worker_id = %worker.record.id, error = %error, "Worker health request failed");
            return ProbeOutcome::Failed {
                worker,
                failure: classify_error(&error, ProbeFailure::Health),
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
                    failure: classify_error(&error, ProbeFailure::Capabilities),
                };
            }
        },
    };
    ProbeOutcome::Healthy { worker, catalog }
}

fn classify_error(error: &VidenoaClientError, fallback: ProbeFailure) -> ProbeFailure {
    if let VidenoaClientError::ClientStatus { reason, .. } = error {
        if error.is_host_not_allowed() {
            return ProbeFailure::HostNotAllowed {
                reason: reason.clone(),
            };
        }
    }
    if error.is_authentication() {
        ProbeFailure::Authentication
    } else if *error == VidenoaClientError::Incompatible {
        ProbeFailure::Incompatible
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authentication_and_incompatibility_are_independent_of_probe_stage() {
        for status in [401, 403] {
            for stage in [ProbeFailure::Health, ProbeFailure::Capabilities] {
                assert_eq!(
                    classify_error(
                        &VidenoaClientError::ClientStatus {
                            status,
                            reason: None,
                            code: None,
                        },
                        stage,
                    ),
                    ProbeFailure::Authentication
                );
            }
        }
        for stage in [ProbeFailure::Health, ProbeFailure::Capabilities] {
            assert_eq!(
                classify_error(&VidenoaClientError::Incompatible, stage),
                ProbeFailure::Incompatible
            );
        }
        for error in [
            VidenoaClientError::Network,
            VidenoaClientError::Timeout,
            VidenoaClientError::Stall,
            VidenoaClientError::MalformedPayload,
            VidenoaClientError::RateLimited,
            VidenoaClientError::ClientStatus {
                status: 429,
                reason: None,
                code: None,
            },
            VidenoaClientError::ServerStatus { status: 503 },
        ] {
            assert_eq!(
                classify_error(&error, ProbeFailure::Health),
                ProbeFailure::Health
            );
            assert_eq!(
                classify_error(&error, ProbeFailure::Capabilities),
                ProbeFailure::Capabilities
            );
        }
    }
    #[test]
    fn incompatibility_has_its_own_health_message() {
        assert_eq!(
            ProbeFailure::Incompatible.message(),
            "worker tunnel is incompatible with this Controller; update the worker or the Controller"
        );
        assert_ne!(
            ProbeFailure::Incompatible.message(),
            ProbeFailure::Health.message()
        );
    }
}
