use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Fault {
    DisconnectBeforeAccept,
    AcceptThenDropRunResponse,
    TruncateDownload { delivered_bytes: usize },
    CorruptOutput { bytes: Vec<u8> },
    DeleteScript(Vec<DeleteOutcome>),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeleteOutcome {
    NotFound,
    ServerError,
    Success,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OfflineMode {
    ConnectionRefused,
    ServiceUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestartMode {
    RetainState,
    LoseState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestartOutcome {
    Retained,
    StateLostAmbiguous,
}

impl RestartOutcome {
    pub const fn requires_manual_reconciliation(self) -> bool {
        match self {
            Self::Retained => false,
            Self::StateLostAmbiguous => true,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct FaultState {
    pub disconnect_before_accept: bool,
    pub accept_then_drop_run_response: bool,
    pub truncate_download: Option<usize>,
    pub corrupt_output: Option<Vec<u8>>,
    pub delete_script: std::collections::VecDeque<DeleteOutcome>,
    pub service_unavailable: bool,
}

impl FaultState {
    pub fn install(&mut self, fault: Fault) {
        match fault {
            Fault::DisconnectBeforeAccept => self.disconnect_before_accept = true,
            Fault::AcceptThenDropRunResponse => self.accept_then_drop_run_response = true,
            Fault::TruncateDownload { delivered_bytes } => {
                self.truncate_download = Some(delivered_bytes);
            }
            Fault::CorruptOutput { bytes } => self.corrupt_output = Some(bytes),
            Fault::DeleteScript(outcomes) => self.delete_script = outcomes.into(),
        }
    }
}
