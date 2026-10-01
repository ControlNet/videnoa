//! Recycling of large per-frame buffers between pipeline stages.
//!
//! A 4K pipeline moves 25-233 MB buffers through every stage. Allocating them
//! fresh per frame makes the allocator map and unmap the pages each time
//! (glibc serves blocks this large with mmap), so every frame pays page
//! faults, kernel zeroing and TLB shootdowns. A pipeline therefore shares one
//! [`FramePool`]: consumers return the buffers of frames they finished with
//! and producers take them back.
//!
//! Buffers handed out by `take_*` keep the contents of their previous use.
//! Callers must overwrite every element they expose downstream.

use std::sync::{Arc, Mutex};

use crate::types::Frame;

/// Maximum number of free buffers kept per element type.
const MAX_FREE_BUFFERS: usize = 32;
/// Maximum number of distinct requested lengths remembered per element type.
const MAX_REQUESTED_LENS: usize = 16;

/// Whether a buffer of `capacity` can serve a request for `len` elements.
fn fits(capacity: usize, len: usize) -> bool {
    (len..=len.saturating_add(len / 4)).contains(&capacity)
}

struct FreeList<T> {
    buffers: Vec<Vec<T>>,
    /// Takes served from `buffers` / by a fresh allocation.
    reused: u64,
    allocated: u64,
    /// Lengths requested from this list, so buffers that no stage asks for
    /// (for example frames from a stage that does not take from the pool)
    /// are freed instead of kept.
    requested_lens: Vec<usize>,
}

/// Free buffers of one element type.
struct VecPool<T> {
    free: Mutex<FreeList<T>>,
}

impl<T: Copy + Default> VecPool<T> {
    fn new() -> Self {
        Self {
            free: Mutex::new(FreeList {
                buffers: Vec::new(),
                reused: 0,
                allocated: 0,
                requested_lens: Vec::new(),
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FreeList<T>> {
        self.free.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// A buffer of exactly `len` elements, reused when a free buffer of a
    /// similar size (capacity within 25% above `len`) exists.
    fn take(&self, len: usize) -> Vec<T> {
        let reused = {
            let mut free = self.lock();
            if !free.requested_lens.contains(&len) {
                if free.requested_lens.len() == MAX_REQUESTED_LENS {
                    free.requested_lens.remove(0);
                }
                free.requested_lens.push(len);
            }
            let reused = free
                .buffers
                .iter()
                .enumerate()
                .filter(|(_, buffer)| fits(buffer.capacity(), len))
                .min_by_key(|(_, buffer)| buffer.capacity())
                .map(|(index, _)| index)
                .map(|index| free.buffers.swap_remove(index));
            match reused {
                Some(_) => free.reused += 1,
                None => free.allocated += 1,
            }
            reused
        };
        match reused {
            Some(mut buffer) => {
                buffer.resize(len, T::default());
                buffer
            }
            None => vec![T::default(); len],
        }
    }

    fn recycle(&self, buffer: Vec<T>) {
        if buffer.capacity() == 0 {
            return;
        }
        let mut free = self.lock();
        let requested = free
            .requested_lens
            .iter()
            .any(|&len| fits(buffer.capacity(), len));
        if requested && free.buffers.len() < MAX_FREE_BUFFERS {
            free.buffers.push(buffer);
        }
    }

    fn clear(&self) {
        self.lock().buffers.clear();
    }

    /// (reused, allocated) take counts.
    fn take_counts(&self) -> (u64, u64) {
        let free = self.lock();
        (free.reused, free.allocated)
    }

    #[cfg(test)]
    fn free_count(&self) -> usize {
        self.lock().buffers.len()
    }
}

/// Shared, bounded free lists of frame buffers for one pipeline.
pub struct FramePool {
    bytes: VecPool<u8>,
    halves: VecPool<u16>,
    floats: VecPool<f32>,
}

impl FramePool {
    pub fn new() -> Self {
        Self {
            bytes: VecPool::new(),
            halves: VecPool::new(),
            floats: VecPool::new(),
        }
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    pub fn take_u8(&self, len: usize) -> Vec<u8> {
        self.bytes.take(len)
    }

    /// FP16 samples stored as raw bits, as in [`Frame::NchwF16`].
    pub fn take_u16(&self, len: usize) -> Vec<u16> {
        self.halves.take(len)
    }

    pub fn take_f32(&self, len: usize) -> Vec<f32> {
        self.floats.take(len)
    }

    pub fn recycle_u8(&self, buffer: Vec<u8>) {
        self.bytes.recycle(buffer);
    }

    pub fn recycle_u16(&self, buffer: Vec<u16>) {
        self.halves.recycle(buffer);
    }

    pub fn recycle_f32(&self, buffer: Vec<f32>) {
        self.floats.recycle(buffer);
    }

    /// Returns the storage of a frame that is no longer needed.
    pub fn recycle_frame(&self, frame: Frame) {
        match frame {
            Frame::CpuRgb { data, .. } => self.recycle_u8(data),
            Frame::NchwF16 { data, .. } => self.recycle_u16(data),
            Frame::NchwF32 { data, .. } | Frame::CpuTensor { data, .. } => self.recycle_f32(data),
        }
    }

    /// Frees every buffer currently in the pool.
    pub fn clear(&self) {
        self.bytes.clear();
        self.halves.clear();
        self.floats.clear();
    }

    #[cfg(test)]
    fn free_counts(&self) -> [usize; 3] {
        [
            self.bytes.free_count(),
            self.halves.free_count(),
            self.floats.free_count(),
        ]
    }
}

/// Test support: buffers that look as if pipeline stages had used the pool.
#[cfg(test)]
impl FramePool {
    /// Leaves a free buffer of `len` copies of `fill` in the pool and returns
    /// its address.
    pub(crate) fn seed_u8(&self, len: usize, fill: u8) -> *const u8 {
        let mut buffer = self.take_u8(len);
        buffer.fill(fill);
        let pointer = buffer.as_ptr();
        self.recycle_u8(buffer);
        pointer
    }

    pub(crate) fn seed_u16(&self, len: usize, fill: u16) -> *const u16 {
        let mut buffer = self.take_u16(len);
        buffer.fill(fill);
        let pointer = buffer.as_ptr();
        self.recycle_u16(buffer);
        pointer
    }

    pub(crate) fn seed_f32(&self, len: usize, fill: f32) -> *const f32 {
        let mut buffer = self.take_f32(len);
        buffer.fill(fill);
        let pointer = buffer.as_ptr();
        self.recycle_f32(buffer);
        pointer
    }

    /// A pooled copy of `values`, as an upstream stage would produce it.
    pub(crate) fn copy_u8(&self, values: &[u8]) -> Vec<u8> {
        let mut buffer = self.take_u8(values.len());
        buffer.copy_from_slice(values);
        buffer
    }

    pub(crate) fn copy_u16(&self, values: &[u16]) -> Vec<u16> {
        let mut buffer = self.take_u16(values.len());
        buffer.copy_from_slice(values);
        buffer
    }

    pub(crate) fn copy_f32(&self, values: &[f32]) -> Vec<f32> {
        let mut buffer = self.take_f32(values.len());
        buffer.copy_from_slice(values);
        buffer
    }
}

impl Default for FramePool {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for FramePool {
    fn drop(&mut self) {
        let (bytes_reused, bytes_allocated) = self.bytes.take_counts();
        let (halves_reused, halves_allocated) = self.halves.take_counts();
        let (floats_reused, floats_allocated) = self.floats.take_counts();
        if bytes_reused
            + bytes_allocated
            + halves_reused
            + halves_allocated
            + floats_reused
            + floats_allocated
            > 0
        {
            tracing::info!(
                u8_reused = bytes_reused,
                u8_allocated = bytes_allocated,
                f16_reused = halves_reused,
                f16_allocated = halves_allocated,
                f32_reused = floats_reused,
                f32_allocated = floats_allocated,
                "Frame pool summary"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recycled_buffer_is_reused_for_a_same_size_request() {
        let pool = FramePool::new();
        let buffer = pool.take_f32(1000);
        let pointer = buffer.as_ptr();
        pool.recycle_f32(buffer);

        let reused = pool.take_f32(1000);
        assert_eq!(reused.as_ptr(), pointer);
        assert_eq!(reused.len(), 1000);
        assert_eq!(pool.free_counts(), [0, 0, 0]);
    }

    #[test]
    fn slightly_smaller_request_reuses_and_shrinks_the_buffer() {
        // Cropped and padded 4K FP32 frames differ by under 1%.
        let pool = FramePool::new();
        let padded = pool.take_f32(1003);
        let pointer = padded.as_ptr();
        pool.recycle_f32(padded);

        let cropped = pool.take_f32(995);
        assert_eq!(cropped.as_ptr(), pointer);
        assert_eq!(cropped.len(), 995);
    }

    #[test]
    fn larger_request_grows_a_buffer_within_its_capacity_class() {
        let pool = FramePool::new();
        let mut buffer = pool.take_u16(100);
        buffer.reserve_exact(10);
        buffer.truncate(90);
        let capacity = buffer.capacity();
        pool.recycle_u16(buffer);

        let grown = pool.take_u16(100);
        assert_eq!(grown.len(), 100);
        assert_eq!(grown.capacity(), capacity);
    }

    #[test]
    fn buffers_of_a_different_size_class_are_not_reused() {
        let pool = FramePool::new();
        pool.recycle_u8(pool.take_u8(4000));

        let small = pool.take_u8(1000);
        assert_eq!(
            small.capacity(),
            1000,
            "a 4x larger buffer must not be handed out"
        );
        let large = pool.take_u8(8000);
        assert_eq!(large.len(), 8000);
        assert_eq!(pool.free_counts(), [1, 0, 0]);
    }

    #[test]
    fn best_fitting_buffer_is_chosen() {
        let pool = FramePool::new();
        let (larger, closer) = (pool.take_f32(1200), pool.take_f32(1010));
        pool.recycle_f32(larger);
        pool.recycle_f32(closer);

        assert_eq!(pool.take_f32(1000).capacity(), 1010);
        assert_eq!(pool.take_f32(1000).capacity(), 1200);
    }

    #[test]
    fn buffers_nobody_requested_are_not_kept() {
        // A sink downstream of a stage that allocates its own frames must not
        // fill the pool with buffers no producer will take.
        let pool = FramePool::new();
        pool.take_u8(1000);

        pool.recycle_u8(vec![0; 4000]);
        pool.recycle_u16(vec![0; 1000]);
        assert_eq!(pool.free_counts(), [0, 0, 0]);

        pool.recycle_u8(vec![0; 1100]);
        assert_eq!(pool.free_counts(), [1, 0, 0]);
    }

    #[test]
    fn free_list_is_bounded() {
        let pool = FramePool::new();
        pool.take_u8(16);
        for _ in 0..MAX_FREE_BUFFERS + 5 {
            pool.recycle_u8(vec![0; 16]);
        }
        assert_eq!(pool.free_counts(), [MAX_FREE_BUFFERS, 0, 0]);
    }

    #[test]
    fn recycle_frame_routes_storage_by_variant() {
        let pool = FramePool::new();
        pool.recycle_frame(Frame::CpuRgb {
            data: pool.take_u8(12),
            width: 2,
            height: 2,
            bit_depth: 8,
        });
        pool.recycle_frame(Frame::NchwF16 {
            data: pool.take_u16(12),
            height: 2,
            width: 2,
        });
        pool.recycle_frame(Frame::NchwF32 {
            data: pool.take_f32(12),
            height: 2,
            width: 2,
        });
        assert_eq!(pool.free_counts(), [1, 1, 1]);
    }

    #[test]
    fn take_counts_distinguish_reuse_from_allocation() {
        let pool = FramePool::new();
        let first = pool.take_f32(8);
        pool.recycle_f32(first);
        let _reused = pool.take_f32(8);
        let _allocated = pool.take_f32(8);

        assert_eq!(pool.floats.take_counts(), (1, 2));
        assert_eq!(pool.bytes.take_counts(), (0, 0));
    }

    #[test]
    fn clear_frees_all_buffers() {
        let pool = FramePool::new();
        pool.seed_u8(4, 0);
        pool.seed_u16(4, 0);
        pool.seed_f32(4, 0.0);

        pool.clear();

        assert_eq!(pool.free_counts(), [0, 0, 0]);
    }

    #[test]
    fn empty_buffers_are_not_kept() {
        let pool = FramePool::new();
        pool.recycle_u8(pool.take_u8(0));
        assert_eq!(pool.free_counts(), [0, 0, 0]);
    }
}
