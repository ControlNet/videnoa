use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header;
use tokio::io::AsyncRead;
use tokio::sync::watch;
use tokio::time::{sleep_until, Instant};
use tokio_util::io::ReaderStream;

use super::transfer::file_endpoint;
use super::transport::classify_reqwest;
use super::{FileApiPath, UploadReceipt, VidenoaClient, VidenoaClientError};

impl VidenoaClient {
    /// Streams a local reader with an inactivity deadline, not a total upload deadline.
    ///
    /// # Errors
    /// Returns typed local I/O, transport, inactivity, status, bounds, or payload failures.
    pub async fn upload<R>(
        &self,
        path: &FileApiPath,
        size: u64,
        reader: R,
    ) -> Result<UploadReceipt, VidenoaClientError>
    where
        R: AsyncRead + Send + Unpin + 'static,
    {
        let (activity, progress) = watch::channel(Instant::now());
        let local_error = Arc::new(AtomicBool::new(false));
        let read_error = local_error.clone();
        let stream = ReaderStream::with_capacity(reader, self.limits.transfer_chunk_bytes).map(
            move |chunk| {
                match &chunk {
                    Ok(bytes) if !bytes.is_empty() => {
                        activity.send_replace(Instant::now());
                    }
                    Err(_) => read_error.store(true, Ordering::Relaxed),
                    Ok(_) => {}
                }
                chunk
            },
        );
        let request = self
            .http
            .put(self.endpoint(&file_endpoint(path, None))?)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_LENGTH, size)
            .body(reqwest::Body::wrap_stream(stream))
            .send();
        let response = while_active(self.timeouts.stall, progress, request)
            .await?
            .map_err(|error| {
                if local_error.load(Ordering::Relaxed) {
                    VidenoaClientError::LocalIo
                } else {
                    classify_reqwest(&error)
                }
            })?;
        self.json(response).await
    }
}

async fn while_active<T>(
    stall: Duration,
    mut progress: watch::Receiver<Instant>,
    request: impl std::future::Future<Output = T>,
) -> Result<T, VidenoaClientError> {
    tokio::pin!(request);
    let mut body_open = true;
    loop {
        // Coalesce heartbeats without buffering file chunks. Use the actual progress
        // timestamp so delayed watchdog polling cannot manufacture extra activity.
        let deadline = *progress.borrow_and_update() + stall;
        tokio::select! {
            biased;
            result = &mut request => return Ok(result),
            changed = progress.changed(), if body_open => {
                body_open = changed.is_ok();
            }
            () = sleep_until(deadline) => return Err(VidenoaClientError::Stall),
        }
        // After body EOF/drop, retain the last deadline while waiting for headers.
    }
}
