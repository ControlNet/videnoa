use std::time::Duration;

use super::ClientConfigError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteTimeouts {
    pub(crate) connect: Duration,
    pub(crate) request: Duration,
    pub(crate) stall: Duration,
}

impl RemoteTimeouts {
    /// Creates nonzero connect, request, and per-chunk stall timeouts.
    ///
    /// # Errors
    /// Returns [`ClientConfigError`] when any timeout is zero.
    pub fn new(
        connect: Duration,
        request: Duration,
        stall: Duration,
    ) -> Result<Self, ClientConfigError> {
        if connect.is_zero() || request.is_zero() || stall.is_zero() {
            return Err(ClientConfigError::ZeroTimeout);
        }
        Ok(Self {
            connect,
            request,
            stall,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PayloadLimits {
    pub(crate) json_bytes: usize,
    pub(crate) transfer_chunk_bytes: usize,
}

impl PayloadLimits {
    /// Creates nonzero JSON and transfer chunk bounds.
    ///
    /// # Errors
    /// Returns [`ClientConfigError`] when either bound is zero.
    pub const fn new(
        json_bytes: usize,
        transfer_chunk_bytes: usize,
    ) -> Result<Self, ClientConfigError> {
        if json_bytes == 0 || transfer_chunk_bytes == 0 {
            return Err(ClientConfigError::ZeroPayloadLimit);
        }
        Ok(Self {
            json_bytes,
            transfer_chunk_bytes,
        })
    }
}
