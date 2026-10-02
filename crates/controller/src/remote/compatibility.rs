use std::collections::BTreeMap;

use crate::domain::{InvalidWorkflow, WorkflowKind, WorkflowName, WorkflowSummary};

use super::WorkflowInterface;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Compatibility {
    Eligible,
    Incompatible,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompatibilityEntry {
    pub kind: WorkflowKind,
    pub compatibility: Compatibility,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompatibilityCatalog {
    entries: BTreeMap<WorkflowName, CompatibilityEntry>,
    invalid: BTreeMap<WorkflowName, String>,
}

impl CompatibilityCatalog {
    #[must_use]
    pub fn from_entries(
        entries: impl IntoIterator<Item = (WorkflowName, CompatibilityEntry)>,
    ) -> Self {
        Self {
            entries: entries.into_iter().collect(),
            invalid: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn compatibility(&self, name: &WorkflowName) -> Option<Compatibility> {
        self.entries.get(name).map(|entry| entry.compatibility)
    }

    #[must_use]
    pub fn entry(&self, name: &WorkflowName) -> Option<&CompatibilityEntry> {
        self.entries.get(name)
    }

    #[must_use]
    pub fn eligible_workflows(&self) -> Vec<WorkflowSummary> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.compatibility == Compatibility::Eligible)
            .map(|(name, entry)| WorkflowSummary {
                name: name.clone(),
                kind: entry.kind,
            })
            .collect()
    }

    /// Workflows whose interface fits but which the Worker rejects.
    #[must_use]
    pub fn invalid_workflows(&self) -> Vec<InvalidWorkflow> {
        self.invalid
            .iter()
            .filter_map(|(name, reason)| {
                self.entries.get(name).map(|entry| InvalidWorkflow {
                    name: name.clone(),
                    kind: entry.kind,
                    reason: reason.clone(),
                })
            })
            .collect()
    }

    /// Names of the entries that are currently eligible.
    pub(crate) fn eligible_names(&self) -> Vec<WorkflowName> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.compatibility == Compatibility::Eligible)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Excludes an entry the Worker would reject, keeping its reason.
    pub(crate) fn mark_invalid(&mut self, name: &WorkflowName, reason: String) {
        if let Some(entry) = self.entries.get_mut(name) {
            entry.compatibility = Compatibility::Incompatible;
            self.invalid.insert(name.clone(), reason);
        }
    }

    pub(crate) fn insert(
        &mut self,
        name: WorkflowName,
        kind: WorkflowKind,
        interface: Option<&WorkflowInterface>,
    ) {
        self.entries
            .entry(name)
            .or_insert_with(|| CompatibilityEntry {
                kind,
                compatibility: interface.map_or(Compatibility::Incompatible, eligibility),
            });
    }
}

fn eligibility(interface: &WorkflowInterface) -> Compatibility {
    let required = ["input", "output"];
    if required.into_iter().all(|required_name| {
        interface
            .inputs
            .iter()
            .any(|port| port.name == required_name && port.port_type == "Path")
    }) {
        Compatibility::Eligible
    } else {
        Compatibility::Incompatible
    }
}
