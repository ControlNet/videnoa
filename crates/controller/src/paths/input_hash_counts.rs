//! Test-only, per-path full-hash counts; no instrumentation is compiled into production.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

static COUNTS: LazyLock<Mutex<HashMap<PathBuf, usize>>> = LazyLock::new(Mutex::default);

pub(super) fn record(path: &Path) {
    *COUNTS
        .lock()
        .expect("hash counters")
        .entry(path.to_owned())
        .or_default() += 1;
}

pub(crate) fn count(path: &Path) -> usize {
    COUNTS
        .lock()
        .expect("hash counters")
        .get(path)
        .copied()
        .unwrap_or(0)
}
