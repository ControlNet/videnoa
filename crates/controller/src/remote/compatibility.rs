use std::collections::{BTreeMap, BTreeSet};

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

    /// Excludes presets a task cannot reach. The Worker's `/api/run` looks a
    /// run name up among saved workflows before presets, so a saved workflow
    /// with a preset's run name runs in its place.
    pub(crate) fn exclude_shadowed_presets(&mut self) {
        let workflows: BTreeSet<&str> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.kind == WorkflowKind::Workflow)
            .map(|(name, _)| name.run_name())
            .collect();
        let shadowed: Vec<WorkflowName> = self
            .entries
            .iter()
            .filter(|(name, entry)| {
                entry.kind == WorkflowKind::Preset && workflows.contains(name.run_name())
            })
            .map(|(name, _)| name.clone())
            .collect();
        for name in shadowed {
            self.mark_invalid(&name, SHADOWED_PRESET.to_owned());
        }
    }

    /// Lists an entry the Worker reports but this Controller cannot read,
    /// unless an entry of that name is already known.
    pub(crate) fn insert_invalid(&mut self, name: WorkflowName, kind: WorkflowKind, reason: &str) {
        if self.entries.contains_key(&name) {
            return;
        }
        self.entries.insert(
            name.clone(),
            CompatibilityEntry {
                kind,
                compatibility: Compatibility::Incompatible,
            },
        );
        self.invalid.insert(name, reason.to_owned());
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

const SHADOWED_PRESET: &str =
    "a saved workflow with the same name runs instead of this preset; rename that workflow on the worker";

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
