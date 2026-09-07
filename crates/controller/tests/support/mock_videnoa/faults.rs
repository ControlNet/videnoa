use serde::{Deserialize, Serialize};

use super::journal::Route;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Fault {
    DisconnectBeforeAccept,
    AcceptThenDropRunResponse,
    TruncateDownload { delivered_bytes: usize },
    CorruptOutput { bytes: Vec<u8> },
    DeleteScript(Vec<DeleteOutcome>),
    Response(ResponseFault),
    StallDownload,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResponseFault {
    pub route: Route,
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeleteOutcome {
    ClientError,
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
    pub response_scripts:
        std::collections::BTreeMap<Route, std::collections::VecDeque<ResponseFault>>,
    pub stall_download: Option<()>,
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
            Fault::Response(response) => self
                .response_scripts
                .entry(response.route)
                .or_default()
                .push_back(response),
            Fault::StallDownload => self.stall_download = Some(()),
        }
    }
}
