use std::sync::atomic::AtomicUsize;

use anyhow::bail;

use super::tests::{
    clone_frame, first_channel_value, sample_frame, update_max, CollectingSink, SharedSinkState,
};
use super::*;

struct SceneAwareInterpolator;

impl FrameInterpolator for SceneAwareInterpolator {
    fn stage_name(&self) -> &str {
        "scene_aware_interpolator"
    }

    fn interpolate(
        &mut self,
        previous: &Frame,
        current: &Frame,
        is_scene_change: bool,
        _ctx: &ExecutionContext,
    ) -> Result<Vec<Frame>> {
        let source = if is_scene_change { previous } else { current };
        Ok(vec![clone_frame(source)?])
    }
}

struct DelayedDuplicateInterpolator {
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
    fail_on_value: Option<u8>,
}

impl DelayedDuplicateInterpolator {
    fn new(active: Arc<AtomicUsize>, max_active: Arc<AtomicUsize>) -> Self {
        Self {
            active,
            max_active,
            fail_on_value: None,
        }
    }

    fn fail_on(mut self, value: u8) -> Self {
        self.fail_on_value = Some(value);
        self
    }
}

impl FrameInterpolator for DelayedDuplicateInterpolator {
    fn stage_name(&self) -> &str {
        "delayed_duplicate_interpolator"
    }

    fn interpolate(
        &mut self,
        previous: &Frame,
        _current: &Frame,
        _is_scene_change: bool,
        _ctx: &ExecutionContext,
    ) -> Result<Vec<Frame>> {
        let value = first_channel_value(previous)?;
        if self.fail_on_value == Some(value) {
            bail!("injected interpolation failure at frame {value}");
        }

        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        update_max(&self.max_active, active);
        std::thread::sleep(Duration::from_millis(if value == 0 { 30 } else { 5 }));
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok(vec![clone_frame(previous)?])
    }
}

#[tokio::test]
async fn test_parallel_interpolator_preserves_order_and_bounds_work() -> Result<()> {
    // Given
    let executor = StreamingExecutor::new(4);
    let frames = (0_u8..6).map(sample_frame).map(Ok);
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let stages = vec![PipelineStage::ParallelInterpolator(vec![
        Box::new(DelayedDuplicateInterpolator::new(
            Arc::clone(&active),
            Arc::clone(&max_active),
        )),
        Box::new(DelayedDuplicateInterpolator::new(
            Arc::clone(&active),
            Arc::clone(&max_active),
        )),
        Box::new(DelayedDuplicateInterpolator::new(
            active,
            Arc::clone(&max_active),
        )),
    ])];
    let state = SharedSinkState::new();
    let sink = CollectingSink::new(state.clone());
    let (_cancel_tx, cancel_rx) = watch::channel(false);

    // When
    executor
        .execute_pipeline_stages(
            frames,
            stages,
            sink,
            PipelineFrameCounts::new(Some(6), Some(11)),
            cancel_rx,
            None,
        )
        .await?;

    // Then
    assert_eq!(state.values(), vec![0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5]);
    assert_eq!(max_active.load(Ordering::SeqCst), 3);
    Ok(())
}

#[test]
fn test_parallel_interpolator_preserves_scene_change_behavior() -> Result<()> {
    // Given
    let (input_tx, input_rx) = mpsc::channel(4);
    let (output_tx, mut output_rx) = mpsc::channel(8);
    let first = IndexedFrame::new(0, sample_frame(10));
    let mut scene_change = IndexedFrame::new(1, sample_frame(20));
    scene_change.is_scene_change = true;
    let third = IndexedFrame::new(2, sample_frame(30));
    input_tx
        .blocking_send(first)
        .map_err(|_| anyhow!("failed to send first frame"))?;
    input_tx
        .blocking_send(scene_change)
        .map_err(|_| anyhow!("failed to send scene-change frame"))?;
    input_tx
        .blocking_send(third)
        .map_err(|_| anyhow!("failed to send third frame"))?;
    drop(input_tx);

    // When
    run_parallel_interpolator_loop(ParallelInterpolatorRun {
        interpolators: vec![
            Box::new(SceneAwareInterpolator),
            Box::new(SceneAwareInterpolator),
        ],
        input: input_rx,
        output: output_tx,
        total_frames: Some(3),
        cancel_state: Arc::new(AtomicBool::new(false)),
        stage_name: "scene_aware_interpolator".to_string(),
    })?;

    // Then
    let mut values = Vec::new();
    while let Some(frame) = output_rx.blocking_recv() {
        values.push(first_channel_value(&frame.frame)?);
    }
    assert_eq!(values, vec![10, 10, 20, 30, 30]);
    Ok(())
}

#[tokio::test]
async fn test_parallel_interpolator_propagates_worker_error() -> Result<()> {
    // Given
    let executor = StreamingExecutor::new(4);
    let frames = (0_u8..8).map(sample_frame).map(Ok);
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let stages = vec![PipelineStage::ParallelInterpolator(vec![
        Box::new(DelayedDuplicateInterpolator::new(
            Arc::clone(&active),
            Arc::clone(&max_active),
        )),
        Box::new(DelayedDuplicateInterpolator::new(active, max_active).fail_on(3)),
    ])];
    let state = SharedSinkState::new();
    let sink = CollectingSink::new(state);
    let (_cancel_tx, cancel_rx) = watch::channel(false);

    // When
    let result = executor
        .execute_pipeline_stages(
            frames,
            stages,
            sink,
            PipelineFrameCounts::new(Some(8), Some(15)),
            cancel_rx,
            None,
        )
        .await;

    // Then
    let error = result.expect_err("parallel interpolator worker failure should stop pipeline");
    let message = format!("{error:#}");
    assert!(message.contains("parallel interpolator stage"), "{message}");
    assert!(
        message.contains("injected interpolation failure at frame 3"),
        "{message}"
    );
    Ok(())
}
