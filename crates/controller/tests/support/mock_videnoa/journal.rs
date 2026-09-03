use std::collections::BTreeMap;

use axum::http::{HeaderMap, Method};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

pub(crate) fn sanitize_entries(entries: &[JournalEntry]) -> Vec<JournalEntry> {
    let mut sanitized = entries.to_vec();
    for entry in &mut sanitized {
        entry.path = match String::from_utf8(redact_uuids(entry.path.as_bytes())) {
            Ok(path) => path,
            Err(_) => entry.path.clone(),
        };
        entry.body = redact_uuids(&entry.body);
        for header in &mut entry.headers {
            if header.name.eq_ignore_ascii_case("idempotency-key") {
                header.value = HeaderValueSnapshot::Redacted;
            }
        }
    }
    sanitized
}

fn redact_uuids(input: &[u8]) -> Vec<u8> {
    const UUID_LENGTH: usize = 36;
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        let candidate = input.get(index..index.saturating_add(UUID_LENGTH));
        let is_uuid = candidate
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .is_some_and(|text| Uuid::parse_str(text).is_ok());
        if is_uuid {
            output.extend_from_slice(b"{id}");
            index += UUID_LENGTH;
        } else {
            output.push(input[index]);
            index += 1;
        }
    }
    output
}
