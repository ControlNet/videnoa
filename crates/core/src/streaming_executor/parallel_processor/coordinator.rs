use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc as std_mpsc, Arc};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use tokio::sync::mpsc;

use super::metrics::StageMetrics;
use super::{ProcessorCompletion, ProcessorJob};
use crate::streaming_executor::IndexedFrame;

pub(super) struct CoordinatorChannels<'a> {
    pub(super) input: &'a mut mpsc::Receiver<IndexedFrame>,
    pub(super) output: &'a mpsc::Sender<IndexedFrame>,
    pub(super) job_senders: &'a [std_mpsc::SyncSender<ProcessorJob>],
    pub(super) completion_rx: &'a std_mpsc::Receiver<ProcessorCompletion>,
}

pub(super) struct CoordinatorControl<'a> {
    pub(super) cancel_state: Arc<AtomicBool>,
    pub(super) stage_name: &'a str,
}

pub(super) struct FrameCoordinator<'a> {
    channels: CoordinatorChannels<'a>,
    control: CoordinatorControl<'a>,
    free_lanes: VecDeque<usize>,
    completions: BTreeMap<u64, ProcessorCompletion>,
    next_job_id: u64,
    next_output_job_id: u64,
    in_flight: usize,
    input_closed: bool,
    output_closed: bool,
    metrics: StageMetrics,
}

impl<'a> FrameCoordinator<'a> {
    pub(super) fn new(channels: CoordinatorChannels<'a>, control: CoordinatorControl<'a>) -> Self {
        let worker_count = channels.job_senders.len();
        Self {
            channels,
            control,
            free_lanes: (0..worker_count).collect(),
            completions: BTreeMap::new(),
            next_job_id: 0,
            next_output_job_id: 0,
            in_flight: 0,
            input_closed: false,
            output_closed: false,
            metrics: StageMetrics::new(),
        }
    }

    pub(super) fn run(mut self) -> Result<()> {
        loop {
            if self.is_cancelled() || !self.emit_ready_completions()? {
                break;
            }

            self.receive_available_frames()?;
            if self.input_closed && self.in_flight == 0 {
                break;
            }

            if self.in_flight == 0 && !self.input_closed {
                self.receive_blocking_frame()?;
                continue;
            }
            self.receive_completion()?;
        }

        self.log_summary();
        Ok(())
    }

    fn is_cancelled(&self) -> bool {
        self.control.cancel_state.load(Ordering::SeqCst)
    }

    fn emit_ready_completions(&mut self) -> Result<bool> {
        while let Some(completion) = self.completions.remove(&self.next_output_job_id) {
            let lane_id = completion.lane_id;
            self.metrics.worker_ms += completion.elapsed_ms;
            let frame = completion.result?;
            let started = Instant::now();
            let sent = self.channels.output.blocking_send(frame).is_ok();
            self.metrics.record_send(started.elapsed(), sent);
            if !sent {
                self.output_closed = true;
                return Ok(false);
            }
            self.free_lanes.push_back(lane_id);
            self.in_flight = self.in_flight.saturating_sub(1);
            self.metrics.frames_processed = self.metrics.frames_processed.saturating_add(1);
            self.next_output_job_id = self.next_output_job_id.saturating_add(1);
        }
        Ok(true)
    }

    fn receive_available_frames(&mut self) -> Result<()> {
        while !self.free_lanes.is_empty() && !self.input_closed {
            match self.channels.input.try_recv() {
                Ok(frame) => self.dispatch_frame(frame)?,
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => self.input_closed = true,
            }
        }
        Ok(())
    }

    fn receive_blocking_frame(&mut self) -> Result<()> {
        let started = Instant::now();
        match self.channels.input.blocking_recv() {
            Some(frame) => {
                self.metrics.record_receive(started.elapsed());
                self.dispatch_frame(frame)
            }
            None => {
                self.input_closed = true;
                Ok(())
            }
        }
    }

    fn receive_completion(&mut self) -> Result<()> {
        match self
            .channels
            .completion_rx
            .recv_timeout(Duration::from_millis(2))
        {
            Ok(completion) => {
                let job_id = completion.job_id;
                if self.completions.insert(job_id, completion).is_some() {
                    bail!("parallel processor received duplicate frame job {job_id}");
                }
            }
            Err(std_mpsc::RecvTimeoutError::Timeout) => {}
            Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                bail!(
                    "parallel processor workers stopped with {} frames in flight",
                    self.in_flight
                )
            }
        }
        Ok(())
    }

    fn dispatch_frame(&mut self, frame: IndexedFrame) -> Result<()> {
        let lane_id = self
            .free_lanes
            .pop_front()
            .ok_or_else(|| anyhow!("parallel processor has no free lane"))?;
        let sender = self
            .channels
            .job_senders
            .get(lane_id)
            .ok_or_else(|| anyhow!("parallel processor has no worker lane {lane_id}"))?;
        sender
            .send(ProcessorJob {
                job_id: self.next_job_id,
                frame,
            })
            .map_err(|_| anyhow!("parallel processor worker {lane_id} stopped"))?;
        self.next_job_id = self.next_job_id.saturating_add(1);
        self.in_flight = self.in_flight.saturating_add(1);
        self.metrics.max_in_flight = self.metrics.max_in_flight.max(self.in_flight);
        Ok(())
    }

    fn log_summary(&self) {
        self.metrics
            .log(self.control.stage_name, self.channels.job_senders.len());
    }
}
