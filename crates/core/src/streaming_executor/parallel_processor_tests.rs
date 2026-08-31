use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;

use anyhow::bail;

use super::tests::{
    first_channel_value, sample_frame, update_max, CollectingSink, SharedSinkState,
};
use super::*;
use crate::node::{Node, PortDefinition};
use crate::types::PortData;

struct DelayedAddProcessor {
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
    fail_on_value: Option<u8>,
}

impl DelayedAddProcessor {
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

impl Node for DelayedAddProcessor {
    fn node_type(&self) -> &str {
        "delayed_add_processor"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![]
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![]
    }

    fn execute(
        &mut self,
        _inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        Ok(HashMap::new())
    }
}

impl FrameProcessor for DelayedAddProcessor {
    fn process_frame(&mut self, frame: Frame, _ctx: &ExecutionContext) -> Result<Frame> {
        let value = first_channel_value(&frame)?;
        if self.fail_on_value == Some(value) {
            bail!("injected processor failure at frame {value}");
        }

        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        update_max(&self.max_active, active);
        std::thread::sleep(Duration::from_millis(if value == 0 { 30 } else { 5 }));
        self.active.fetch_sub(1, Ordering::SeqCst);

        match frame {
            Frame::CpuRgb {
                mut data,
                width,
                height,
                bit_depth,
            } => {
                for channel in &mut data {
                    *channel = channel.saturating_add(10);
                }
                Ok(Frame::CpuRgb {
                    data,
                    width,
                    height,
                    bit_depth,
                })
            }
            other => Ok(other),
        }
    }
}

struct GatedIdentityProcessor {
    active: Arc<AtomicUsize>,
    release: Arc<AtomicBool>,
}

impl Node for GatedIdentityProcessor {
    fn node_type(&self) -> &str {
        "gated_identity_processor"
    }

    fn input_ports(&self) -> Vec<PortDefinition> {
        vec![]
    }

    fn output_ports(&self) -> Vec<PortDefinition> {
        vec![]
    }

    fn execute(
        &mut self,
        _inputs: &HashMap<String, PortData>,
        _ctx: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        Ok(HashMap::new())
    }
}

impl FrameProcessor for GatedIdentityProcessor {
    fn process_frame(&mut self, frame: Frame, _ctx: &ExecutionContext) -> Result<Frame> {
        self.active.fetch_add(1, Ordering::SeqCst);
        while !self.release.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok(frame)
    }
}

#[tokio::test]
async fn parallel_processor_preserves_order_and_bounds_work() -> Result<()> {
    let executor = StreamingExecutor::new(4);
    let frames = (0_u8..8).map(sample_frame).map(Ok);
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let processors: Vec<Box<dyn FrameProcessor>> = (0..3)
        .map(|_| {
            Box::new(DelayedAddProcessor::new(
                Arc::clone(&active),
                Arc::clone(&max_active),
            )) as Box<dyn FrameProcessor>
        })
        .collect();
    let stages = vec![PipelineStage::ParallelProcessor(processors)];
    let state = SharedSinkState::new();
    let sink = CollectingSink::new(state.clone());
    let (_cancel_tx, cancel_rx) = watch::channel(false);

    executor
        .execute_pipeline_stages(
            frames,
            stages,
            sink,
            PipelineFrameCounts::new(Some(8), Some(8)),
            cancel_rx,
            None,
        )
        .await?;

    assert_eq!(state.values(), vec![10, 11, 12, 13, 14, 15, 16, 17]);
    assert_eq!(max_active.load(Ordering::SeqCst), 3);
    Ok(())
}

#[tokio::test]
async fn parallel_processor_propagates_worker_error() -> Result<()> {
    let executor = StreamingExecutor::new(4);
    let frames = (0_u8..8).map(sample_frame).map(Ok);
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let processors: Vec<Box<dyn FrameProcessor>> = (0..3)
        .map(|_| {
            Box::new(
                DelayedAddProcessor::new(Arc::clone(&active), Arc::clone(&max_active)).fail_on(3),
            ) as Box<dyn FrameProcessor>
        })
        .collect();
    let stages = vec![PipelineStage::ParallelProcessor(processors)];
    let state = SharedSinkState::new();
    let sink = CollectingSink::new(state);
    let (_cancel_tx, cancel_rx) = watch::channel(false);

    let result = executor
        .execute_pipeline_stages(
            frames,
            stages,
            sink,
            PipelineFrameCounts::new(Some(8), Some(8)),
            cancel_rx,
            None,
        )
        .await;

    let error = result.expect_err("parallel processor worker failure should stop pipeline");
    let message = format!("{error:#}");
    assert!(message.contains("parallel processor stage"), "{message}");
    assert!(
        message.contains("injected processor failure at frame 3"),
        "{message}"
    );
    Ok(())
}

#[tokio::test]
async fn parallel_processor_cancels_with_all_lanes_active() -> Result<()> {
    let executor = StreamingExecutor::new(4);
    let frames = (0_u8..8).map(sample_frame).map(Ok);
    let active = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(AtomicBool::new(false));
    let processors: Vec<Box<dyn FrameProcessor>> = (0..3)
        .map(|_| {
            Box::new(GatedIdentityProcessor {
                active: Arc::clone(&active),
                release: Arc::clone(&release),
            }) as Box<dyn FrameProcessor>
        })
        .collect();
    let stages = vec![PipelineStage::ParallelProcessor(processors)];
    let state = SharedSinkState::new();
    let sink = CollectingSink::new(state.clone());
    let (cancel_tx, cancel_rx) = watch::channel(false);
    tokio::spawn(async move {
        while active.load(Ordering::SeqCst) < 3 {
            tokio::task::yield_now().await;
        }
        let _ = cancel_tx.send(true);
        tokio::time::sleep(Duration::from_millis(10)).await;
        release.store(true, Ordering::SeqCst);
    });

    executor
        .execute_pipeline_stages(
            frames,
            stages,
            sink,
            PipelineFrameCounts::new(Some(8), Some(8)),
            cancel_rx,
            None,
        )
        .await?;

    assert!(
        state.values().is_empty(),
        "cancellation must not emit completed frames"
    );
    Ok(())
}
