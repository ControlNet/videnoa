use std::sync::atomic::AtomicUsize;

use super::tests::{
    clone_frame, sample_frame, CollectingSink, DuplicateInterpolator, SharedSinkState,
};
use super::*;

struct GatedDuplicateInterpolator {
    active: Arc<AtomicUsize>,
    release: Arc<AtomicBool>,
}

impl FrameInterpolator for GatedDuplicateInterpolator {
    fn interpolate(
        &mut self,
        previous: &Frame,
        _current: &Frame,
        _is_scene_change: bool,
        _ctx: &ExecutionContext,
    ) -> Result<Vec<Frame>> {
        self.active.fetch_add(1, Ordering::SeqCst);
        while !self.release.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok(vec![clone_frame(previous)?])
    }
}

#[tokio::test]
async fn test_parallel_interpolator_handles_empty_input() -> Result<()> {
    // Given
    let executor = StreamingExecutor::new(4);
    let stages = vec![PipelineStage::ParallelInterpolator(vec![
        Box::new(DuplicateInterpolator),
        Box::new(DuplicateInterpolator),
    ])];
    let state = SharedSinkState::new();
    let sink = CollectingSink::new(state.clone());
    let (_cancel_tx, cancel_rx) = watch::channel(false);

    // When
    executor
        .execute_pipeline_stages(
            std::iter::empty::<Result<Frame>>(),
            stages,
            sink,
            Some(0),
            Some(0),
            cancel_rx,
            None,
        )
        .await?;

    // Then
    assert!(state.values().is_empty());
    Ok(())
}

#[tokio::test]
async fn test_parallel_interpolator_emits_single_input_frame() -> Result<()> {
    // Given
    let executor = StreamingExecutor::new(4);
    let stages = vec![PipelineStage::ParallelInterpolator(vec![
        Box::new(DuplicateInterpolator),
        Box::new(DuplicateInterpolator),
    ])];
    let state = SharedSinkState::new();
    let sink = CollectingSink::new(state.clone());
    let (_cancel_tx, cancel_rx) = watch::channel(false);

    // When
    executor
        .execute_pipeline_stages(
            std::iter::once(Ok(sample_frame(7))),
            stages,
            sink,
            Some(1),
            Some(1),
            cancel_rx,
            None,
        )
        .await?;

    // Then
    assert_eq!(state.values(), vec![7]);
    Ok(())
}

#[tokio::test]
async fn test_parallel_interpolator_cancels_with_both_lanes_active() -> Result<()> {
    // Given
    let executor = StreamingExecutor::new(4);
    let frames = (0_u8..6).map(sample_frame).map(Ok);
    let active = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(AtomicBool::new(false));
    let stages = vec![PipelineStage::ParallelInterpolator(vec![
        Box::new(GatedDuplicateInterpolator {
            active: Arc::clone(&active),
            release: Arc::clone(&release),
        }),
        Box::new(GatedDuplicateInterpolator {
            active: Arc::clone(&active),
            release: Arc::clone(&release),
        }),
    ])];
    let state = SharedSinkState::new();
    let sink = CollectingSink::new(state.clone());
    let (cancel_tx, cancel_rx) = watch::channel(false);
    tokio::spawn(async move {
        while active.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
        let _ = cancel_tx.send(true);
        tokio::time::sleep(Duration::from_millis(10)).await;
        release.store(true, Ordering::SeqCst);
    });

    // When
    executor
        .execute_pipeline_stages(frames, stages, sink, Some(6), Some(11), cancel_rx, None)
        .await?;

    // Then
    assert!(
        state.values().is_empty(),
        "cancellation must not emit completed or trailing frames"
    );
    Ok(())
}
