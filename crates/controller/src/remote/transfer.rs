use reqwest::header;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio_util::io::ReaderStream;

use super::transport::{classify_reqwest, classify_response_body, ensure_success, stalled};
use super::{
    DownloadReceipt, FileApiPath, FileStat, UploadReceipt, VidenoaClient, VidenoaClientError,
};

impl VidenoaClient {
    /// Streams a local reader into one remote workspace file.
    ///
    /// # Errors
    /// Returns [`VidenoaClientError`] for local I/O, transport, status, bounds, or payload failures.
    pub async fn upload<R>(
        &self,
        path: &FileApiPath,
        size: u64,
        reader: R,
    ) -> Result<UploadReceipt, VidenoaClientError>
    where
        R: AsyncRead + Send + Unpin + 'static,
    {
        let segments = file_endpoint(path, None);
        let stream = ReaderStream::with_capacity(reader, self.limits.transfer_chunk_bytes);
        let response = self
            .http
            .put(self.endpoint(&segments)?)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_LENGTH, size)
            .body(reqwest::Body::wrap_stream(stream))
            .timeout(self.timeouts.request)
            .send()
            .await
            .map_err(|error| {
                if error.is_body() {
                    VidenoaClientError::LocalIo
                } else {
                    classify_reqwest(&error)
                }
            })?;
        self.json(response).await
    }

    /// Streams one remote file into the provided writer.
    ///
    /// # Errors
    /// Returns [`VidenoaClientError`] for local I/O, transport, status, stall, or truncation.
    pub async fn download<W>(
        &self,
        path: &FileApiPath,
        writer: &mut W,
    ) -> Result<DownloadReceipt, VidenoaClientError>
    where
        W: AsyncWrite + Unpin,
    {
        let response = self
            .http
            .get(self.endpoint(&file_endpoint(path, None))?)
            .timeout(self.timeouts.request)
            .send()
            .await
            .map_err(|error| classify_download_start(&error))?;
        ensure_success(response.status())?;
        self.copy_download(response, writer).await
    }

    /// Fetches typed metadata for one remote workspace path.
    ///
    /// # Errors
    /// Returns [`VidenoaClientError`] for transport, status, bounds, or payload failures.
    pub async fn stat(&self, path: &FileApiPath) -> Result<FileStat, VidenoaClientError> {
        let response = self
            .send(
                self.http
                    .get(self.endpoint(&file_endpoint(path, Some("stat")))?),
            )
            .await?;
        self.json(response).await
    }

    /// Deletes one remote workspace file or directory.
    ///
    /// # Errors
    /// Returns [`VidenoaClientError`] for transport or typed status failures.
    pub async fn delete_file(&self, path: &FileApiPath) -> Result<(), VidenoaClientError> {
        let response = self
            .send(self.http.delete(self.endpoint(&file_endpoint(path, None))?))
            .await?;
        ensure_success(response.status())
    }

    async fn copy_download<W>(
        &self,
        mut response: reqwest::Response,
        writer: &mut W,
    ) -> Result<DownloadReceipt, VidenoaClientError>
    where
        W: AsyncWrite + Unpin,
    {
        let expected = response
            .content_length()
            .ok_or(VidenoaClientError::MalformedPayload)?;
        let mut bytes = 0_u64;
        loop {
            let chunk = stalled(self.timeouts.stall, response.chunk())
                .await?
                .map_err(|error| classify_response_body(&error))?;
            let Some(chunk) = chunk else {
                break;
            };
            for part in chunk.chunks(self.limits.transfer_chunk_bytes) {
                writer
                    .write_all(part)
                    .await
                    .map_err(|_| VidenoaClientError::LocalIo)?;
            }
            bytes = bytes
                .checked_add(
                    u64::try_from(chunk.len()).map_err(|_| VidenoaClientError::MalformedPayload)?,
                )
                .ok_or(VidenoaClientError::MalformedPayload)?;
        }
        if expected != bytes {
            return Err(VidenoaClientError::MalformedPayload);
        }
        Ok(DownloadReceipt { bytes })
    }
}

fn classify_download_start(error: &reqwest::Error) -> VidenoaClientError {
    if error.is_timeout() {
        VidenoaClientError::Timeout
    } else if error.is_connect() {
        VidenoaClientError::Network
    } else {
        VidenoaClientError::MalformedPayload
    }
}

fn file_endpoint<'a>(path: &'a FileApiPath, suffix: Option<&'a str>) -> Vec<&'a str> {
    let mut segments = vec!["api", "files"];
    segments.extend(path.segments());
    if let Some(suffix) = suffix {
        segments.push(suffix);
    }
    segments
}
