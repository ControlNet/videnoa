use std::collections::BTreeMap;
use std::time::Duration;

use tokio::sync::{watch, Mutex};

use super::journal::LogicalTimestamp;
use super::state::HarnessError;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Checkpoint {
    BeforeAcceptingUpload,
    AfterUploadBytesAccepted,
    BeforeRunPersistence,
    AfterRunPersistedBeforeResponse,
    BeforePollResponse,
    BeforeDownloadBody,
    MidDownloadBody,
    BeforeDelete,
    AfterDelete,
}

impl Checkpoint {
    pub const fn name(self) -> &'static str {
        match self {
            Self::BeforeAcceptingUpload => "before_accepting_upload",
            Self::AfterUploadBytesAccepted => "after_upload_bytes_accepted",
            Self::BeforeRunPersistence => "before_run_persistence",
            Self::AfterRunPersistedBeforeResponse => "after_run_persisted_before_response",
            Self::BeforePollResponse => "before_poll_response",
            Self::BeforeDownloadBody => "before_download_body",
            Self::MidDownloadBody => "mid_download_body",
            Self::BeforeDelete => "before_delete",
            Self::AfterDelete => "after_delete",
        }
    }

    const fn all() -> [Self; 9] {
        [
            Self::BeforeAcceptingUpload,
            Self::AfterUploadBytesAccepted,
            Self::BeforeRunPersistence,
            Self::AfterRunPersistedBeforeResponse,
            Self::BeforePollResponse,
            Self::BeforeDownloadBody,
            Self::MidDownloadBody,
            Self::BeforeDelete,
            Self::AfterDelete,
        ]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckpointTicket {
    checkpoint: Checkpoint,
    generation: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct GateState {
    generation: u64,
    reached: u64,
    released: u64,
}

struct Gate {
    state: Mutex<GateState>,
    changes: watch::Sender<GateState>,
}

pub(crate) struct CheckpointHub {
    gates: BTreeMap<Checkpoint, Gate>,
}

impl CheckpointHub {
    pub fn new() -> Self {
        let gates = Checkpoint::all()
            .into_iter()
            .map(|checkpoint| {
                let (changes, _) = watch::channel(GateState::default());
                (
                    checkpoint,
                    Gate {
                        state: Mutex::new(GateState::default()),
                        changes,
                    },
                )
            })
            .collect();
        Self { gates }
    }

    pub async fn pause(&self, checkpoint: Checkpoint) -> CheckpointTicket {
        let gate = &self.gates[&checkpoint];
        let mut state = gate.state.lock().await;
        state.generation += 1;
        gate.changes.send_replace(*state);
        CheckpointTicket {
            checkpoint,
            generation: state.generation,
        }
    }

    pub async fn arrive(
        &self,
        checkpoint: Checkpoint,
        timestamp: LogicalTimestamp,
    ) -> LogicalTimestamp {
        let gate = &self.gates[&checkpoint];
        let generation = {
            let mut state = gate.state.lock().await;
            let generation = state.generation;
            state.reached = state.reached.max(generation);
            gate.changes.send_replace(*state);
            generation
        };
        if generation == 0 {
            return timestamp;
        }
        let mut changes = gate.changes.subscribe();
        loop {
            if changes.borrow().released >= generation {
                return timestamp;
            }
            if changes.changed().await.is_err() {
                return timestamp;
            }
        }
    }

    pub async fn await_reached(&self, ticket: &CheckpointTicket) -> Result<(), HarnessError> {
        let gate = &self.gates[&ticket.checkpoint];
        let mut changes = gate.changes.subscribe();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if changes.borrow().reached >= ticket.generation {
                    return Ok(());
                }
                changes
                    .changed()
                    .await
                    .map_err(|_| HarnessError::CheckpointClosed)?;
            }
        })
        .await
        .map_err(|_| HarnessError::CheckpointTimeout(ticket.checkpoint.name()))?
    }

    pub async fn release(&self, ticket: CheckpointTicket) -> Result<(), HarnessError> {
        let gate = &self.gates[&ticket.checkpoint];
        let mut state = gate.state.lock().await;
        if ticket.generation > state.generation {
            return Err(HarnessError::UnknownCheckpointGeneration);
        }
        state.released = state.released.max(ticket.generation);
        gate.changes.send_replace(*state);
        Ok(())
    }
}
