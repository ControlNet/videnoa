use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio::sync::{mpsc, watch};

use crate::node::{ExecutionContext, FrameProcessor};
use crate::types::Frame;

pub const DEFAULT_BUFFER_SIZE: usize = 4;

pub struct IndexedFrame {
    pub index: u64,
    pub timestamp: Option<Duration>,
    pub frame: Frame,
    pub is_scene_change: bool,
}

impl IndexedFrame {
    pub fn new(index: u64, frame: Frame) -> Self {
        Self {
            index,
            timestamp: None,
            frame,
            is_scene_change: false,
        }
    }
}

pub trait FrameSink: Send + 'static {
    fn write_frame(&mut self, frame: &Frame) -> Result<()>;
    fn finish(&mut self) -> Result<()>;
}

pub trait FrameInterpolator: Send + 'static {
    fn stage_name(&self) -> &str {
        "FrameInterpolator"
    }

    fn interpolate(
        &mut self,
        previous: &Frame,
        current: &Frame,
        is_scene_change: bool,
        ctx: &ExecutionContext,
    ) -> Result<Vec<Frame>>;
}

impl<F> FrameInterpolator for F
where
    F: FnMut(&Frame, &Frame, bool, &ExecutionContext) -> Result<Vec<Frame>> + Send + 'static,
{
    fn interpolate(
        &mut self,
        previous: &Frame,
        current: &Frame,
        is_scene_change: bool,
        ctx: &ExecutionContext,
    ) -> Result<Vec<Frame>> {
        self(previous, current, is_scene_change, ctx)
    }
}

pub enum PipelineStage {
    Processor(Box<dyn FrameProcessor>),
    Interpolator(Box<dyn FrameInterpolator>),
}

pub struct StreamingExecutor {
    buffer_size: usize,
}

impl StreamingExecutor {
    pub fn new(buffer_size: usize) -> Self {
        Self {
            buffer_size: buffer_size.max(1),
        }
    }

    pub async fn execute_pipeline<D, E>(
        &self,
        decoder: D,
        processors: Vec<Box<dyn FrameProcessor>>,
        encoder: E,
        total_frames: Option<u64>,
        cancel: watch::Receiver<bool>,
        progress_callback: Option<Box<dyn Fn(u64, Option<u64>, Option<u64>) + Send>>,
    ) -> Result<()>
    where
        D: Iterator<Item = Result<Frame>> + Send + 'static,
        E: FrameSink,
    {
        let stages = processors
            .into_iter()
            .map(PipelineStage::Processor)
            .collect();

        self.execute_pipeline_stages(
            decoder,
            stages,
            encoder,
            total_frames,
            total_frames,
            cancel,
            progress_callback,
        )
        .await
    }

    pub async fn execute_pipeline_stages<D, E>(
        &self,
        decoder: D,
        stages: Vec<PipelineStage>,
        encoder: E,
        total_frames: Option<u64>,
        total_output_frames: Option<u64>,
        cancel: watch::Receiver<bool>,
        progress_callback: Option<Box<dyn Fn(u64, Option<u64>, Option<u64>) + Send>>,
    ) -> Result<()>
    where
        D: Iterator<Item = Result<Frame>> + Send + 'static,
        E: FrameSink,
    {
        if *cancel.borrow() {
            return Ok(());
        }

        let (error_tx, mut error_rx) = mpsc::unbounded_channel::<anyhow::Error>();
        let (cancel_tx, _) = watch::channel(false);
        let cancel_state = Arc::new(AtomicBool::new(false));

        let external_cancel_handle =
            spawn_external_cancel_watcher(cancel, cancel_state.clone(), cancel_tx.clone());

        let mut handles = Vec::new();

        let (first_tx, first_rx) = mpsc::channel(self.buffer_size);
        handles.push(spawn_decoder_stage(
            decoder,
            first_tx,
            cancel_state.clone(),
            cancel_tx.clone(),
            error_tx.clone(),
        ));

        let mut upstream_rx = first_rx;
        for stage in stages {
            let (next_tx, next_rx) = mpsc::channel(self.buffer_size);

            match stage {
                PipelineStage::Processor(processor) => {
                    handles.push(spawn_processor_stage(
                        processor,
                        upstream_rx,
                        next_tx,
                        total_frames,
                        cancel_state.clone(),
                        cancel_tx.clone(),
                        error_tx.clone(),
                    ));
                }
                PipelineStage::Interpolator(interpolator) => {
                    handles.push(spawn_interpolator_stage(
                        interpolator,
                        upstream_rx,
                        next_tx,
                        total_frames,
                        cancel_state.clone(),
                        cancel_tx.clone(),
                        error_tx.clone(),
                    ));
                }
            }

            upstream_rx = next_rx;
        }

        handles.push(spawn_encoder_stage(
            encoder,
            upstream_rx,
            total_output_frames,
            total_frames,
            progress_callback,
            cancel_state.clone(),
            cancel_tx.clone(),
            error_tx.clone(),
        ));

        drop(error_tx);

        let mut first_error: Option<anyhow::Error> = None;

        for handle in handles {
            match handle.await {
                Ok(()) => {}
                Err(join_error) => {
                    signal_cancel(&cancel_state, &cancel_tx);
                    if first_error.is_none() {
                        first_error = Some(anyhow!("streaming task panicked: {join_error}"));
                    }
                }
            }
        }

        while let Some(error) = error_rx.recv().await {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }

        signal_cancel(&cancel_state, &cancel_tx);
        external_cancel_handle.abort();
        if let Err(join_error) = external_cancel_handle.await {
            if !join_error.is_cancelled() && first_error.is_none() {
                first_error = Some(anyhow!("external cancel watcher failed: {join_error}"));
            }
        }

        if let Some(error) = first_error {
            return Err(error);
        }

        Ok(())
    }
}

impl Default for StreamingExecutor {
    fn default() -> Self {
        Self::new(DEFAULT_BUFFER_SIZE)
    }
}

fn spawn_external_cancel_watcher(
    mut cancel: watch::Receiver<bool>,
    cancel_state: Arc<AtomicBool>,
    cancel_tx: watch::Sender<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if *cancel.borrow() {
            signal_cancel(&cancel_state, &cancel_tx);
            return;
        }

        loop {
            match cancel.changed().await {
                Ok(()) => {
                    if *cancel.borrow() {
                        signal_cancel(&cancel_state, &cancel_tx);
                        return;
                    }
                }
                Err(_) => {
                    return;
                }
            }
        }
    })
}

fn spawn_decoder_stage<D>(
    mut decoder: D,
    output: mpsc::Sender<IndexedFrame>,
    cancel_state: Arc<AtomicBool>,
    cancel_tx: watch::Sender<bool>,
    error_tx: mpsc::UnboundedSender<anyhow::Error>,
) -> tokio::task::JoinHandle<()>
where
    D: Iterator<Item = Result<Frame>> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let result = run_decoder_loop(&mut decoder, output, cancel_state.clone());
        if let Err(error) = result {
            report_task_error(
                &error_tx,
                &cancel_state,
                &cancel_tx,
                error.context("decoder stage failed"),
            );
        }
    })
}

fn spawn_processor_stage(
    mut processor: Box<dyn FrameProcessor>,
    input: mpsc::Receiver<IndexedFrame>,
    output: mpsc::Sender<IndexedFrame>,
    total_frames: Option<u64>,
    cancel_state: Arc<AtomicBool>,
    cancel_tx: watch::Sender<bool>,
    error_tx: mpsc::UnboundedSender<anyhow::Error>,
) -> tokio::task::JoinHandle<()> {
    let stage_name = processor.node_type().to_string();
    tokio::task::spawn_blocking(move || {
        let result = run_processor_loop(
            &mut processor,
            input,
            output,
            total_frames,
            cancel_state.clone(),
            &stage_name,
        );
        if let Err(error) = result {
            report_task_error(
                &error_tx,
                &cancel_state,
                &cancel_tx,
                error.context(format!("processor stage '{stage_name}' failed")),
            );
        }
    })
}

fn spawn_interpolator_stage(
    mut interpolator: Box<dyn FrameInterpolator>,
    input: mpsc::Receiver<IndexedFrame>,
    output: mpsc::Sender<IndexedFrame>,
    total_frames: Option<u64>,
    cancel_state: Arc<AtomicBool>,
    cancel_tx: watch::Sender<bool>,
    error_tx: mpsc::UnboundedSender<anyhow::Error>,
) -> tokio::task::JoinHandle<()> {
    let stage_name = interpolator.stage_name().to_string();
    tokio::task::spawn_blocking(move || {
        let result = run_interpolator_loop(
            &mut interpolator,
            input,
            output,
            total_frames,
            cancel_state.clone(),
            &stage_name,
        );
        if let Err(error) = result {
            report_task_error(
                &error_tx,
                &cancel_state,
                &cancel_tx,
                error.context(format!("interpolator stage '{stage_name}' failed")),
            );
        }
    })
}

fn spawn_encoder_stage<E>(
    mut encoder: E,
    input: mpsc::Receiver<IndexedFrame>,
    total_output_frames: Option<u64>,
    total_input_frames: Option<u64>,
    progress_callback: Option<Box<dyn Fn(u64, Option<u64>, Option<u64>) + Send>>,
    cancel_state: Arc<AtomicBool>,
    cancel_tx: watch::Sender<bool>,
    error_tx: mpsc::UnboundedSender<anyhow::Error>,
) -> tokio::task::JoinHandle<()>
where
    E: FrameSink,
{
    tokio::task::spawn_blocking(move || {
        let result = run_encoder_loop(
            &mut encoder,
            input,
            total_output_frames,
            total_input_frames,
            progress_callback,
            cancel_state.clone(),
        );

        match result {
            Ok(()) => {
                let finish_result = encoder.finish().context("encoder finish failed");
                if let Err(error) = finish_result {
                    if cancel_state.load(Ordering::SeqCst) {
                        return;
                    }

                    report_task_error(
                        &error_tx,
                        &cancel_state,
                        &cancel_tx,
                        error.context("encoder stage failed while finalizing"),
                    );
                }
            }
            Err(error) => {
                report_task_error(
                    &error_tx,
                    &cancel_state,
                    &cancel_tx,
                    error.context("encoder stage failed"),
                );
            }
        }
    })
}

fn run_decoder_loop<D>(
    decoder: &mut D,
    output: mpsc::Sender<IndexedFrame>,
    cancel_state: Arc<AtomicBool>,
) -> Result<()>
where
    D: Iterator<Item = Result<Frame>>,
{
    let mut index = 0_u64;
    let mut total_decode_ms = 0.0_f64;
    let mut total_send_ms = 0.0_f64;

    for frame_result in decoder {
        if cancel_state.load(Ordering::SeqCst) {
            break;
        }

        let t_decode = std::time::Instant::now();
        let frame = frame_result.with_context(|| format!("failed to decode frame {index}"))?;
        total_decode_ms += t_decode.elapsed().as_secs_f64() * 1000.0;

        let indexed_frame = IndexedFrame::new(index, frame);

        let t_send = std::time::Instant::now();
        if output.blocking_send(indexed_frame).is_err() {
            break;
        }
        total_send_ms += t_send.elapsed().as_secs_f64() * 1000.0;

        index = index.saturating_add(1);
    }

    if index > 0 {
        tracing::info!(
            frames = index,
            avg_decode_ms = format!("{:.1}", total_decode_ms / index as f64),
            avg_send_wait_ms = format!("{:.1}", total_send_ms / index as f64),
            total_decode_ms = format!("{:.0}", total_decode_ms),
            total_send_wait_ms = format!("{:.0}", total_send_ms),
            "Decoder stage summary"
        );
    }

    Ok(())
}

fn run_processor_loop(
    processor: &mut Box<dyn FrameProcessor>,
    mut input: mpsc::Receiver<IndexedFrame>,
    output: mpsc::Sender<IndexedFrame>,
    total_frames: Option<u64>,
    cancel_state: Arc<AtomicBool>,
    stage_name: &str,
) -> Result<()> {
    let mut ctx = ExecutionContext {
        total_frames,
        current_frame: 0,
        ..Default::default()
    };
    let mut frame_count = 0_u64;
    let mut total_recv_ms = 0.0_f64;
    let mut total_process_ms = 0.0_f64;
    let mut total_send_ms = 0.0_f64;

    loop {
        if cancel_state.load(Ordering::SeqCst) {
            break;
        }

        let t_recv = std::time::Instant::now();
        let Some(mut indexed_frame) = input.blocking_recv() else {
            break;
        };
        total_recv_ms += t_recv.elapsed().as_secs_f64() * 1000.0;

        ctx.current_frame = indexed_frame.index;
        let frame_index = indexed_frame.index;

        let t_process = std::time::Instant::now();
        indexed_frame.frame = processor
            .process_frame(indexed_frame.frame, &ctx)
            .with_context(|| format!("processor '{stage_name}' failed on frame {frame_index}"))?;
        total_process_ms += t_process.elapsed().as_secs_f64() * 1000.0;

        let t_send = std::time::Instant::now();
        if output.blocking_send(indexed_frame).is_err() {
            break;
        }
        total_send_ms += t_send.elapsed().as_secs_f64() * 1000.0;

        frame_count += 1;
    }

    if frame_count > 0 {
        tracing::info!(
            frames = frame_count,
            avg_recv_wait_ms = format!("{:.1}", total_recv_ms / frame_count as f64),
            avg_process_ms = format!("{:.1}", total_process_ms / frame_count as f64),
            avg_send_wait_ms = format!("{:.1}", total_send_ms / frame_count as f64),
            total_process_ms = format!("{:.0}", total_process_ms),
            total_recv_wait_ms = format!("{:.0}", total_recv_ms),
            total_send_wait_ms = format!("{:.0}", total_send_ms),
            stage = stage_name,
            "Processor stage summary"
        );
    }

    Ok(())
}

fn run_interpolator_loop(
    interpolator: &mut Box<dyn FrameInterpolator>,
    mut input: mpsc::Receiver<IndexedFrame>,
    output: mpsc::Sender<IndexedFrame>,
    total_frames: Option<u64>,
    cancel_state: Arc<AtomicBool>,
    stage_name: &str,
) -> Result<()> {
    let mut ctx = ExecutionContext {
        total_frames,
        current_frame: 0,
        ..Default::default()
    };
    let mut previous: Option<IndexedFrame> = None;
    let mut output_index = 0_u64;
    let mut pairs_processed = 0_u64;
    let mut total_recv_ms = 0.0_f64;
    let mut total_interpolate_ms = 0.0_f64;
    let mut total_send_ms = 0.0_f64;

    loop {
        if cancel_state.load(Ordering::SeqCst) {
            break;
        }

        let t_recv = std::time::Instant::now();
        let Some(current) = input.blocking_recv() else {
            break;
        };
        total_recv_ms += t_recv.elapsed().as_secs_f64() * 1000.0;

        if let Some(prev) = previous.take() {
            ctx.current_frame = prev.index;

            let t_interp = std::time::Instant::now();
            let interpolated_frames = interpolator
                .interpolate(&prev.frame, &current.frame, current.is_scene_change, &ctx)
                .with_context(|| {
                    format!(
                        "interpolator '{stage_name}' failed on pair {} -> {}",
                        prev.index, current.index
                    )
                })?;
            total_interpolate_ms += t_interp.elapsed().as_secs_f64() * 1000.0;
            pairs_processed += 1;

            let prev_timestamp = prev.timestamp;
            let current_timestamp = current.timestamp;

            let previous_output = IndexedFrame {
                index: output_index,
                timestamp: prev_timestamp,
                frame: prev.frame,
                is_scene_change: prev.is_scene_change,
            };

            let t_send = std::time::Instant::now();
            if output.blocking_send(previous_output).is_err() {
                return Ok(());
            }
            total_send_ms += t_send.elapsed().as_secs_f64() * 1000.0;

            output_index = output_index.saturating_add(1);

            let interpolation_count = interpolated_frames.len();
            for (position, frame) in interpolated_frames.into_iter().enumerate() {
                let timestamp = interpolate_timestamp(
                    prev_timestamp,
                    current_timestamp,
                    position + 1,
                    interpolation_count + 1,
                );

                let interpolated = IndexedFrame {
                    index: output_index,
                    timestamp,
                    frame,
                    is_scene_change: current.is_scene_change,
                };

                let t_send2 = std::time::Instant::now();
                if output.blocking_send(interpolated).is_err() {
                    return Ok(());
                }
                total_send_ms += t_send2.elapsed().as_secs_f64() * 1000.0;

                output_index = output_index.saturating_add(1);
            }

            previous = Some(current);
        } else {
            previous = Some(current);
        }
    }

    if !cancel_state.load(Ordering::SeqCst) {
        if let Some(last) = previous {
            let final_frame = IndexedFrame {
                index: output_index,
                timestamp: last.timestamp,
                frame: last.frame,
                is_scene_change: last.is_scene_change,
            };
            let _ = output.blocking_send(final_frame);
        }
    }

    if pairs_processed > 0 {
        tracing::info!(
            pairs = pairs_processed,
            output_frames = output_index,
            avg_recv_wait_ms = format!("{:.1}", total_recv_ms / pairs_processed as f64),
            avg_interpolate_ms = format!("{:.1}", total_interpolate_ms / pairs_processed as f64),
            avg_send_wait_ms = format!("{:.1}", total_send_ms / output_index as f64),
            total_interpolate_ms = format!("{:.0}", total_interpolate_ms),
            total_recv_wait_ms = format!("{:.0}", total_recv_ms),
            total_send_wait_ms = format!("{:.0}", total_send_ms),
            stage = stage_name,
            "Interpolator stage summary"
        );
    }

    Ok(())
}

fn run_encoder_loop<E>(
    encoder: &mut E,
    mut input: mpsc::Receiver<IndexedFrame>,
    total_output_frames: Option<u64>,
    total_input_frames: Option<u64>,
    progress_callback: Option<Box<dyn Fn(u64, Option<u64>, Option<u64>) + Send>>,
    cancel_state: Arc<AtomicBool>,
) -> Result<()>
where
    E: FrameSink,
{
    let mut written = 0_u64;
    let mut total_recv_ms = 0.0_f64;
    let mut total_encode_ms = 0.0_f64;

    loop {
        if cancel_state.load(Ordering::SeqCst) {
            break;
        }

        let t_recv = std::time::Instant::now();
        let Some(indexed_frame) = input.blocking_recv() else {
            break;
        };
        total_recv_ms += t_recv.elapsed().as_secs_f64() * 1000.0;

        let t_enc = std::time::Instant::now();
        encoder
            .write_frame(&indexed_frame.frame)
            .with_context(|| format!("failed to encode frame {}", indexed_frame.index))?;
        total_encode_ms += t_enc.elapsed().as_secs_f64() * 1000.0;

        written = written.saturating_add(1);

        if let Some(callback) = progress_callback.as_ref() {
            callback(written, total_output_frames, total_input_frames);
        }
    }

    if written > 0 {
        tracing::info!(
            frames = written,
            avg_recv_wait_ms = format!("{:.1}", total_recv_ms / written as f64),
            avg_encode_ms = format!("{:.1}", total_encode_ms / written as f64),
            total_encode_ms = format!("{:.0}", total_encode_ms),
            total_recv_wait_ms = format!("{:.0}", total_recv_ms),
            "Encoder stage summary"
        );
    }

    Ok(())
}

fn interpolate_timestamp(
    previous: Option<Duration>,
    current: Option<Duration>,
    position: usize,
    total_segments: usize,
) -> Option<Duration> {
    if position >= total_segments {
        return None;
    }

    let previous = previous?;
    let current = current?;

    if current < previous {
        return Some(previous);
    }

    let delta = current - previous;
    let fraction = position as f64 / total_segments as f64;
    Some(previous + Duration::from_secs_f64(delta.as_secs_f64() * fraction))
}

fn signal_cancel(cancel_state: &Arc<AtomicBool>, cancel_tx: &watch::Sender<bool>) {
    cancel_state.store(true, Ordering::SeqCst);
    let _ = cancel_tx.send(true);
}

fn report_task_error(
    error_tx: &mpsc::UnboundedSender<anyhow::Error>,
    cancel_state: &Arc<AtomicBool>,
    cancel_tx: &watch::Sender<bool>,
    error: anyhow::Error,
) {
    signal_cancel(cancel_state, cancel_tx);
    let _ = error_tx.send(error);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;

    use crate::node::{Node, PortDefinition};
    use crate::types::PortData;
    use anyhow::bail;

    struct AddProcessor {
        name: String,
        addend: u8,
        delay: Duration,
        fail_on_frame: Option<u64>,
    }

    impl AddProcessor {
        fn new(name: &str, addend: u8) -> Self {
            Self {
                name: name.to_string(),
                addend,
                delay: Duration::ZERO,
                fail_on_frame: None,
            }
        }

        fn fail_on(mut self, frame: u64) -> Self {
            self.fail_on_frame = Some(frame);
            self
        }
    }

    impl Node for AddProcessor {
        fn node_type(&self) -> &str {
            &self.name
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

    impl FrameProcessor for AddProcessor {
        fn process_frame(&mut self, frame: Frame, ctx: &ExecutionContext) -> Result<Frame> {
            if self.delay > Duration::ZERO {
                std::thread::sleep(self.delay);
            }

            if self.fail_on_frame == Some(ctx.current_frame) {
                bail!("injected failure at frame {}", ctx.current_frame);
            }

            match frame {
                Frame::CpuRgb {
                    mut data,
                    width,
                    height,
                    bit_depth,
                } => {
                    for value in &mut data {
                        *value = value.saturating_add(self.addend);
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

    struct DuplicateInterpolator;

    impl FrameInterpolator for DuplicateInterpolator {
        fn stage_name(&self) -> &str {
            "duplicate_interpolator"
        }

        fn interpolate(
            &mut self,
            previous: &Frame,
            _current: &Frame,
            _is_scene_change: bool,
            _ctx: &ExecutionContext,
        ) -> Result<Vec<Frame>> {
            Ok(vec![clone_frame(previous)?])
        }
    }

    #[derive(Clone)]
    struct SharedSinkState {
        values: Arc<Mutex<Vec<u8>>>,
        written: Arc<AtomicUsize>,
    }

    impl SharedSinkState {
        fn new() -> Self {
            Self {
                values: Arc::new(Mutex::new(Vec::new())),
                written: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn values(&self) -> Vec<u8> {
            self.values.lock().expect("values mutex poisoned").clone()
        }

        fn written_count(&self) -> usize {
            self.written.load(Ordering::SeqCst)
        }
    }

    struct CollectingSink {
        state: SharedSinkState,
        delay: Duration,
    }

    impl CollectingSink {
        fn new(state: SharedSinkState) -> Self {
            Self {
                state,
                delay: Duration::ZERO,
            }
        }

        fn with_delay(mut self, delay: Duration) -> Self {
            self.delay = delay;
            self
        }
    }

    impl FrameSink for CollectingSink {
        fn write_frame(&mut self, frame: &Frame) -> Result<()> {
            if self.delay > Duration::ZERO {
                std::thread::sleep(self.delay);
            }

            self.state
                .values
                .lock()
                .expect("values mutex poisoned")
                .push(first_channel_value(frame)?);
            self.state.written.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn finish(&mut self) -> Result<()> {
            Ok(())
        }
    }

    struct CountingSource {
        total: u64,
        next_index: u64,
        produced: Arc<AtomicUsize>,
        written: Arc<AtomicUsize>,
        max_lag: Arc<AtomicUsize>,
    }

    impl Iterator for CountingSource {
        type Item = Result<Frame>;

        fn next(&mut self) -> Option<Self::Item> {
            if self.next_index >= self.total {
                return None;
            }

            let produced = self.produced.fetch_add(1, Ordering::SeqCst) + 1;
            let written = self.written.load(Ordering::SeqCst);
            let lag = produced.saturating_sub(written);
            update_max(&self.max_lag, lag);

            let frame = sample_frame(self.next_index as u8);
            self.next_index = self.next_index.saturating_add(1);
            Some(Ok(frame))
        }
    }

    fn sample_frame(value: u8) -> Frame {
        Frame::CpuRgb {
            data: vec![value, value, value],
            width: 1,
            height: 1,
            bit_depth: 8,
        }
    }

    fn clone_frame(frame: &Frame) -> Result<Frame> {
        match frame {
            Frame::CpuRgb {
                data,
                width,
                height,
                bit_depth,
            } => Ok(Frame::CpuRgb {
                data: data.clone(),
                width: *width,
                height: *height,
                bit_depth: *bit_depth,
            }),
            Frame::CpuTensor {
                data,
                channels,
                height,
                width,
            } => Ok(Frame::CpuTensor {
                data: data.clone(),
                channels: *channels,
                height: *height,
                width: *width,
            }),
            Frame::NchwF32 {
                data,
                height,
                width,
            } => Ok(Frame::NchwF32 {
                data: data.clone(),
                height: *height,
                width: *width,
            }),
            Frame::NchwF16 {
                data,
                height,
                width,
            } => Ok(Frame::NchwF16 {
                data: data.clone(),
                height: *height,
                width: *width,
            }),
        }
    }

    fn first_channel_value(frame: &Frame) -> Result<u8> {
        match frame {
            Frame::CpuRgb { data, .. } => data
                .first()
                .copied()
                .ok_or_else(|| anyhow!("rgb frame payload is empty")),
            Frame::CpuTensor { .. } | Frame::NchwF32 { .. } | Frame::NchwF16 { .. } => {
                bail!("expected rgb frame")
            }
        }
    }

    fn update_max(target: &Arc<AtomicUsize>, value: usize) {
        let mut current = target.load(Ordering::SeqCst);
        while value > current {
            match target.compare_exchange(current, value, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => return,
                Err(new_current) => current = new_current,
            }
        }
    }

    #[tokio::test]
    async fn test_frames_flow_through_three_stage_pipeline() {
        let executor = StreamingExecutor::new(4);
        let frames = (0_u8..10).map(sample_frame).map(Ok);

        let processors: Vec<Box<dyn FrameProcessor>> = vec![
            Box::new(AddProcessor::new("add_1", 1)),
            Box::new(AddProcessor::new("add_2", 1)),
            Box::new(AddProcessor::new("add_3", 1)),
        ];

        let state = SharedSinkState::new();
        let sink = CollectingSink::new(state.clone());
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        executor
            .execute_pipeline(frames, processors, sink, Some(10), cancel_rx, None)
            .await
            .expect("pipeline should complete");

        let values = state.values();
        assert_eq!(values.len(), 10);
        for (index, value) in values.into_iter().enumerate() {
            assert_eq!(value, index as u8 + 3);
        }
    }

    #[tokio::test]
    async fn test_backpressure_limits_in_flight_frames() {
        let executor = StreamingExecutor::new(1);
        let state = SharedSinkState::new();
        let max_lag = Arc::new(AtomicUsize::new(0));
        let produced = Arc::new(AtomicUsize::new(0));

        let source = CountingSource {
            total: 40,
            next_index: 0,
            produced: produced.clone(),
            written: state.written.clone(),
            max_lag: max_lag.clone(),
        };

        let sink = CollectingSink::new(state.clone()).with_delay(Duration::from_millis(5));
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        executor
            .execute_pipeline(source, Vec::new(), sink, Some(40), cancel_rx, None)
            .await
            .expect("pipeline should complete");

        assert_eq!(produced.load(Ordering::SeqCst), 40);
        assert_eq!(state.written_count(), 40);

        let observed_max_lag = max_lag.load(Ordering::SeqCst);
        assert!(
            observed_max_lag <= 3,
            "expected bounded backpressure, observed lag={observed_max_lag}"
        );
    }

    #[tokio::test]
    async fn test_error_in_middle_stage_stops_pipeline() {
        let executor = StreamingExecutor::new(2);
        let frames = (0_u8..20).map(sample_frame).map(Ok);

        let processors: Vec<Box<dyn FrameProcessor>> = vec![
            Box::new(AddProcessor::new("pre", 1)),
            Box::new(AddProcessor::new("failing", 1).fail_on(7)),
        ];

        let state = SharedSinkState::new();
        let sink = CollectingSink::new(state.clone());
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let result = executor
            .execute_pipeline(frames, processors, sink, Some(20), cancel_rx, None)
            .await;

        assert!(result.is_err(), "pipeline should fail");
        let error_message = result.err().expect("should be Err").to_string();
        assert!(
            error_message.contains("processor stage 'failing' failed"),
            "unexpected error message: {error_message}"
        );
        assert!(state.written_count() < 20);
    }

    #[tokio::test]
    async fn test_cancel_signal_stops_pipeline() {
        let executor = StreamingExecutor::new(4);
        let frames = (0_u64..10_000).map(|index| Ok(sample_frame((index % 255) as u8)));

        let state = SharedSinkState::new();
        let sink = CollectingSink::new(state.clone()).with_delay(Duration::from_millis(2));

        let (cancel_tx, cancel_rx) = watch::channel(false);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(40)).await;
            let _ = cancel_tx.send(true);
        });

        executor
            .execute_pipeline(frames, Vec::new(), sink, None, cancel_rx, None)
            .await
            .expect("canceled pipeline should exit cleanly");

        assert!(
            state.written_count() < 10_000,
            "cancel should stop processing before completion"
        );
    }

    #[tokio::test]
    async fn test_fi_style_interpolator_outputs_expected_frame_count() {
        let executor = StreamingExecutor::new(4);
        let frames = (0_u8..10).map(sample_frame).map(Ok);

        let stages = vec![PipelineStage::Interpolator(Box::new(DuplicateInterpolator))];
        let state = SharedSinkState::new();
        let sink = CollectingSink::new(state.clone());
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        executor
            .execute_pipeline_stages(frames, stages, sink, Some(10), Some(19), cancel_rx, None)
            .await
            .expect("pipeline with interpolator should complete");

        let values = state.values();
        assert_eq!(values.len(), 19);

        for pair in 0..9 {
            assert_eq!(values[pair * 2], pair as u8);
            assert_eq!(values[pair * 2 + 1], pair as u8);
        }
        assert_eq!(values[18], 9);
    }

    #[tokio::test]
    async fn test_progress_callback_reports_encoded_frames() {
        let executor = StreamingExecutor::new(4);
        let frames = (0_u8..6).map(sample_frame).map(Ok);

        let progress_state = Arc::new(Mutex::new(Vec::new()));
        let progress_state_clone = progress_state.clone();
        let callback: Box<dyn Fn(u64, Option<u64>, Option<u64>) + Send> =
            Box::new(move |current, total_output, total_input| {
                progress_state_clone
                    .lock()
                    .expect("progress mutex poisoned")
                    .push((current, total_output, total_input));
            });

        let sink_state = SharedSinkState::new();
        let sink = CollectingSink::new(sink_state);
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        executor
            .execute_pipeline(frames, Vec::new(), sink, Some(6), cancel_rx, Some(callback))
            .await
            .expect("pipeline should complete");

        let progress = progress_state
            .lock()
            .expect("progress mutex poisoned")
            .clone();
        assert_eq!(progress.len(), 6);
        assert_eq!(progress.first(), Some(&(1, Some(6), Some(6))));
        assert_eq!(progress.last(), Some(&(6, Some(6), Some(6))));
    }

    #[test]
    fn test_interpolated_timestamp_is_linear() {
        let interpolated = interpolate_timestamp(
            Some(Duration::from_secs(0)),
            Some(Duration::from_secs(1)),
            1,
            2,
        )
        .expect("interpolated timestamp should exist");

        assert_eq!(interpolated, Duration::from_millis(500));
    }

    #[test]
    fn test_interpolated_timestamp_handles_missing_values() {
        assert!(interpolate_timestamp(None, Some(Duration::from_secs(1)), 1, 2).is_none());
        assert!(interpolate_timestamp(Some(Duration::from_secs(0)), None, 1, 2).is_none());
    }
}
