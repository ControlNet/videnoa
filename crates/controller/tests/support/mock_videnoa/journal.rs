use std::collections::BTreeMap;

use axum::http::{HeaderMap, Method};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    Health,
    Workflows,
    Presets,
    Interface,
    Upload,
    Run,
    JobPoll,
    JobCancel,
    Download,
    Stat,
    DeleteFile,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct LogicalTimestamp(pub u64);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum HeaderValueSnapshot {
    Bytes(Vec<u8>),
    Redacted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JournalHeader {
    pub name: String,
    pub value: HeaderValueSnapshot,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum JournalOutcome {
    Delivered,
    TransportDropped,
    Truncated {
        advertised_bytes: usize,
        delivered_bytes: usize,
    },
    CorruptOutput,
    FaultStatus,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JournalEntry {
    pub sequence: u64,
    pub method: String,
    pub path: String,
    pub headers: Vec<JournalHeader>,
    pub body: Vec<u8>,
    pub response_status: u16,
    pub route: Route,
    pub checkpoints: BTreeMap<String, LogicalTimestamp>,
    pub outcome: JournalOutcome,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RouteCounters(BTreeMap<Route, u64>);

impl RouteCounters {
    pub const fn empty() -> Self {
        Self(BTreeMap::new())
    }

    pub fn increment(&mut self, route: Route) {
        *self.0.entry(route).or_default() += 1;
    }

    pub fn get(&self, route: Route) -> u64 {
        self.0.get(&route).copied().unwrap_or_default()
    }
}

pub(crate) fn snapshot_headers(headers: &HeaderMap) -> Vec<JournalHeader> {
    let mut output = Vec::new();
    for name in headers.keys() {
        let redacted = matches!(
            name.as_str(),
            "authorization" | "cookie" | "host" | "set-cookie" | "x-csrf-token"
        );
        for value in headers.get_all(name) {
            output.push(JournalHeader {
                name: name.as_str().to_owned(),
                value: if redacted {
                    HeaderValueSnapshot::Redacted
                } else {
                    HeaderValueSnapshot::Bytes(value.as_bytes().to_vec())
                },
            });
        }
    }
    output.sort_by(|left, right| left.name.cmp(&right.name));
    output
}

pub(crate) struct JournalRequest {
    pub sequence: u64,
    pub method: Method,
    pub path: String,
    pub headers: Vec<JournalHeader>,
    pub body: Vec<u8>,
    pub route: Route,
    pub checkpoints: BTreeMap<String, LogicalTimestamp>,
}

impl JournalRequest {
    pub fn finish(self, response_status: u16, outcome: JournalOutcome) -> JournalEntry {
        JournalEntry {
            sequence: self.sequence,
            method: self.method.to_string(),
            path: self.path,
            headers: self.headers,
            body: self.body,
            response_status,
            route: self.route,
            checkpoints: self.checkpoints,
            outcome,
        }
    }
}
