use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::domain::{SchedulerStatus, WorkerId};

use super::TransferLimitError;

#[derive(Debug)]
struct TransferState {
    upload_limit: u16,
    download_limit: u16,
    uploads: u16,
    downloads: u16,
    uploading_workers: BTreeSet<WorkerId>,
}

#[derive(Clone, Debug)]
pub struct TransferCoordinator {
    state: Arc<Mutex<TransferState>>,
}

#[derive(Debug)]
pub struct UploadPermit {
    state: Arc<Mutex<TransferState>>,
    worker_id: WorkerId,
}

#[derive(Debug)]
pub struct DownloadPermit {
    state: Arc<Mutex<TransferState>>,
}

impl TransferCoordinator {
    /// Creates independent upload/download pools and a per-worker upload guard.
    ///
    /// # Errors
    /// Returns [`TransferLimitError`] when either limit is zero.
    pub fn new(upload_limit: u16, download_limit: u16) -> Result<Self, TransferLimitError> {
        if upload_limit == 0 || download_limit == 0 {
            return Err(TransferLimitError);
        }
        Ok(Self {
            state: Arc::new(Mutex::new(TransferState {
                upload_limit,
                download_limit,
                uploads: 0,
                downloads: 0,
                uploading_workers: BTreeSet::new(),
            })),
        })
    }

    pub(crate) fn from_status(status: &SchedulerStatus) -> Self {
        Self {
            state: Arc::new(Mutex::new(TransferState {
                upload_limit: status.max_concurrent_uploads.get(),
                download_limit: status.max_concurrent_downloads.get(),
                uploads: 0,
                downloads: 0,
                uploading_workers: BTreeSet::new(),
            })),
        }
    }

    pub(crate) fn reconfigure(&self, status: &SchedulerStatus) {
        let mut state = lock(&self.state);
        state.upload_limit = status.max_concurrent_uploads.get();
        state.download_limit = status.max_concurrent_downloads.get();
    }

    #[must_use]
    pub fn try_upload(&self, worker_id: WorkerId) -> Option<UploadPermit> {
        let mut state = lock(&self.state);
        if state.uploads >= state.upload_limit || state.uploading_workers.contains(&worker_id) {
            return None;
        }
        state.uploads += 1;
        state.uploading_workers.insert(worker_id);
        Some(UploadPermit {
            state: Arc::clone(&self.state),
            worker_id,
        })
    }

    #[must_use]
    pub fn try_download(&self) -> Option<DownloadPermit> {
        let mut state = lock(&self.state);
        if state.downloads >= state.download_limit {
            return None;
        }
        state.downloads += 1;
        Some(DownloadPermit {
            state: Arc::clone(&self.state),
        })
    }
}

impl Drop for UploadPermit {
    fn drop(&mut self) {
        let mut state = lock(&self.state);
        state.uploads = state.uploads.saturating_sub(1);
        state.uploading_workers.remove(&self.worker_id);
    }
}

impl Drop for DownloadPermit {
    fn drop(&mut self) {
        let mut state = lock(&self.state);
        state.downloads = state.downloads.saturating_sub(1);
    }
}

fn lock(state: &Mutex<TransferState>) -> MutexGuard<'_, TransferState> {
    match state.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
