use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc as std_mpsc, Arc};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use tokio::sync::mpsc;

use super::metrics::StageMetrics;
use super::{PairCompletion, PairJob};
use crate::streaming_executor::{interpolate_timestamp, IndexedFrame};

pub(super) struct CoordinatorChannels<'a> {
    pub(super) input: &'a mut mpsc::Receiver<IndexedFrame>,
    pub(super) output: &'a mpsc::Sender<IndexedFrame>,
    pub(super) job_senders: &'a [std_mpsc::SyncSender<PairJob>],
    pub(super) completion_rx: &'a std_mpsc::Receiver<PairCompletion>,
}

pub(super) struct CoordinatorControl<'a> {
    pub(super) cancel_state: Arc<AtomicBool>,
    pub(super) stage_name: &'a str,
}

pub(super) struct PairCoordinator<'a> {
    channels: CoordinatorChannels<'a>,
    control: CoordinatorControl<'a>,
    previous: Option<Arc<IndexedFrame>>,
    free_lanes: VecDeque<usize>,
    completions: BTreeMap<u64, PairCompletion>,
    next_pair_id: u64,
    next_output_pair_id: u64,
    in_flight: usize,
    input_closed: bool,
    output_closed: bool,
    metrics: StageMetrics,
}

impl<'a> PairCoordinator<'a> {
    pub(super) fn new(channels: CoordinatorChannels<'a>, control: CoordinatorControl<'a>) -> Self {
        let worker_count = channels.job_senders.len();
        Self {
            channels,
            control,
            previous: None,
            free_lanes: (0..worker_count).collect(),
            completions: BTreeMap::new(),
            next_pair_id: 0,
            next_output_pair_id: 0,
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

        self.emit_final_frame()?;
        self.log_summary();
        Ok(())
    }

    fn is_cancelled(&self) -> bool {
        self.control.cancel_state.load(Ordering::SeqCst)
    }

    fn emit_ready_completions(&mut self) -> Result<bool> {
        while let Some(completion) = self.completions.remove(&self.next_output_pair_id) {
            let lane_id = completion.lane_id;
            self.metrics.worker_ms += completion.elapsed_ms;
            if !self.emit_completion(completion)? {
                self.output_closed = true;
                return Ok(false);
            }
            self.free_lanes.push_back(lane_id);
            self.in_flight = self.in_flight.saturating_sub(1);
            self.metrics.pairs_processed = self.metrics.pairs_processed.saturating_add(1);
            self.next_output_pair_id = self.next_output_pair_id.saturating_add(1);
        }
        Ok(true)
    }

    fn receive_available_frames(&mut self) -> Result<()> {
        while (!self.free_lanes.is_empty() || self.previous.is_none()) && !self.input_closed {
            match self.channels.input.try_recv() {
                Ok(current) => self.dispatch_frame(current)?,
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => self.input_closed = true,
            }
        }
        Ok(())
    }

    fn receive_blocking_frame(&mut self) -> Result<()> {
        let started = Instant::now();
        match self.channels.input.blocking_recv() {
            Some(current) => {
                self.metrics.record_receive(started.elapsed());
                self.dispatch_frame(current)
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
                let pair_id = completion.pair_id;
                if self.completions.insert(pair_id, completion).is_some() {
                    bail!("parallel interpolator received duplicate pair {pair_id}");
                }
            }
            Err(std_mpsc::RecvTimeoutError::Timeout) => {}
            Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                bail!(
                    "parallel interpolator workers stopped with {} pairs in flight",
                    self.in_flight
                )
            }
        }
        Ok(())
    }

    fn dispatch_frame(&mut self, current: IndexedFrame) -> Result<()> {
        let current = Arc::new(current);
        let Some(previous) = self.previous.replace(Arc::clone(&current)) else {
            return Ok(());
        };
        let lane_id = self
            .free_lanes
            .pop_front()
            .ok_or_else(|| anyhow!("parallel interpolator has no free lane"))?;
        let sender = self
            .channels
            .job_senders
            .get(lane_id)
            .ok_or_else(|| anyhow!("parallel interpolator has no worker lane {lane_id}"))?;
        sender
            .send(PairJob {
                pair_id: self.next_pair_id,
                previous,
                current,
            })
            .map_err(|_| anyhow!("parallel interpolator worker {lane_id} stopped"))?;
        self.next_pair_id = self.next_pair_id.saturating_add(1);
        self.in_flight = self.in_flight.saturating_add(1);
        self.metrics.max_in_flight = self.metrics.max_in_flight.max(self.in_flight);
        Ok(())
    }

    fn emit_completion(&mut self, completion: PairCompletion) -> Result<bool> {
        let interpolated_frames = completion.result?;
        let previous_timestamp = completion.previous.timestamp;
        let current_timestamp = completion.current.timestamp;
        let is_scene_change = completion.current.is_scene_change;
        let previous = Arc::try_unwrap(completion.previous).map_err(|_| {
            anyhow!(
                "parallel interpolator retained previous frame for pair {}",
                completion.pair_id
            )
        })?;
        drop(completion.current);

        let previous_output = IndexedFrame {
            index: self.metrics.output_frames(),
            timestamp: previous_timestamp,
            frame: previous.frame,
            is_scene_change: previous.is_scene_change,
        };
        if !self.send_output(previous_output) {
            return Ok(false);
        }

        let interpolation_count = interpolated_frames.len();
        for (position, frame) in interpolated_frames.into_iter().enumerate() {
            let interpolated = IndexedFrame {
                index: self.metrics.output_frames(),
                timestamp: interpolate_timestamp(
                    previous_timestamp,
                    current_timestamp,
                    position + 1,
                    interpolation_count + 1,
                ),
                frame,
                is_scene_change,
            };
            if !self.send_output(interpolated) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn send_output(&mut self, frame: IndexedFrame) -> bool {
        let started = Instant::now();
        let sent = self.channels.output.blocking_send(frame).is_ok();
        self.metrics.record_send(started.elapsed(), sent);
        sent
    }

    fn emit_final_frame(&mut self) -> Result<()> {
        if self.is_cancelled() || self.output_closed {
            return Ok(());
        }
        let Some(last) = self.previous.take() else {
            return Ok(());
        };
        let last = Arc::try_unwrap(last)
            .map_err(|_| anyhow!("parallel interpolator retained the final frame"))?;
        let final_frame = IndexedFrame {
            index: self.metrics.output_frames(),
            timestamp: last.timestamp,
            frame: last.frame,
            is_scene_change: last.is_scene_change,
        };
        let _ = self.send_output(final_frame);
        Ok(())
    }

    fn log_summary(&self) {
        self.metrics
            .log(self.control.stage_name, self.channels.job_senders.len());
    }
}
