use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use sha2::{Digest, Sha256};
use tokio::io::AsyncWrite;

use crate::persistence::Sha256Digest;

pub(super) struct HashingWriter<W> {
    writer: W,
    hasher: Sha256,
    bytes: u64,
}

impl<W> HashingWriter<W> {
    pub(super) fn new(writer: W) -> Self {
        Self {
            writer,
            hasher: Sha256::new(),
            bytes: 0,
        }
    }

    pub(super) fn finish(self) -> (W, u64, Sha256Digest) {
        let digest: [u8; 32] = self.hasher.finalize().into();
        (self.writer, self.bytes, Sha256Digest::new(digest))
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for HashingWriter<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match Pin::new(&mut self.writer).poll_write(context, buffer) {
            Poll::Ready(Ok(written)) => {
                self.hasher.update(&buffer[..written]);
                self.bytes = match self.bytes.checked_add(
                    u64::try_from(written)
                        .map_err(|_| io::Error::other("download byte count exceeds u64"))?,
                ) {
                    Some(bytes) => bytes,
                    None => return Poll::Ready(Err(io::Error::other("download byte overflow"))),
                };
                Poll::Ready(Ok(written))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.writer).poll_flush(context)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.writer).poll_shutdown(context)
    }
}
