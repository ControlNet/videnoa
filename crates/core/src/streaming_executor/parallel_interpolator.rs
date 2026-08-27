use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc as std_mpsc, Arc};
use std::thread;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use tokio::sync::mpsc;

use super::{ExecutionContext, Frame, FrameInterpolator, IndexedFrame};
use coordinator::{CoordinatorChannels, CoordinatorControl, PairCoordinator};

mod coordinator;
#[cfg(test)]
mod join_tests;
mod metrics;

pub(super) struct ParallelInterpolatorRun {
    pub(super) interpolators: Vec<Box<dyn FrameInterpolator>>,
    pub(super) input: mpsc::Receiver<IndexedFrame>,
    pub(super) output: mpsc::Sender<IndexedFrame>,
    pub(super) total_frames: Option<u64>,
    pub(super) cancel_state: Arc<AtomicBool>,
    pub(super) stage_name: String,
}

pub(super) fn run_parallel_interpolator_loop(run: ParallelInterpolatorRun) -> Result<()> {
    let ParallelInterpolatorRun {
        interpolators,
        mut input,
        output,
        total_frames,
        cancel_state,
        stage_name,
    } = run;
    if interpolators.len() != 2 {
        bail!(
            "parallel interpolator requires exactly two lanes, got {}",
            interpolators.len()
        );
    }

    let WorkerPool {
        job_senders,
        completion_rx,
        handles,
    } = spawn_workers(interpolators, total_frames, &stage_name);
    let channels = CoordinatorChannels {
        input: &mut input,
        output: &output,
        job_senders: &job_senders,
        completion_rx: &completion_rx,
    };
    let control = CoordinatorControl {
        cancel_state,
        stage_name: &stage_name,
    };
    let run_result = PairCoordinator::new(channels, control).run();

    drop(job_senders);
    join_workers(handles, run_result)
}

struct PairJob {
    pair_id: u64,
    previous: Arc<IndexedFrame>,
    current: Arc<IndexedFrame>,
}

struct PairCompletion {
    pair_id: u64,
    lane_id: usize,
    previous: Arc<IndexedFrame>,
    current: Arc<IndexedFrame>,
    result: Result<Vec<Frame>>,
    elapsed_ms: f64,
}

struct WorkerPool {
    job_senders: Vec<std_mpsc::SyncSender<PairJob>>,
    completion_rx: std_mpsc::Receiver<PairCompletion>,
    handles: Vec<thread::JoinHandle<()>>,
}

fn spawn_workers(
    interpolators: Vec<Box<dyn FrameInterpolator>>,
    total_frames: Option<u64>,
    stage_name: &str,
) -> WorkerPool {
    let (completion_tx, completion_rx) = std_mpsc::channel();
    let mut job_senders = Vec::with_capacity(interpolators.len());
    let mut handles = Vec::with_capacity(interpolators.len());

    for (lane_id, mut interpolator) in interpolators.into_iter().enumerate() {
        let (job_tx, job_rx) = std_mpsc::sync_channel::<PairJob>(1);
        let lane_completion_tx = completion_tx.clone();
        let lane_stage_name = stage_name.to_string();
        job_senders.push(job_tx);
        handles.push(thread::spawn(move || {
            while let Ok(job) = job_rx.recv() {
                let ctx = ExecutionContext {
                    total_frames,
                    current_frame: job.previous.index,
                    ..Default::default()
                };
                let started = Instant::now();
                let result = match catch_unwind(AssertUnwindSafe(|| {
                    interpolator.interpolate(
                        &job.previous.frame,
                        &job.current.frame,
                        job.current.is_scene_change,
                        &ctx,
                    )
                })) {
                    Ok(result) => result.with_context(|| {
                        format!(
                            "interpolator '{lane_stage_name}' failed on pair {} -> {}",
                            job.previous.index, job.current.index
                        )
                    }),
                    Err(_) => Err(anyhow!(
                        "interpolator '{lane_stage_name}' panicked on pair {} -> {}",
                        job.previous.index,
                        job.current.index
                    )),
                };
                let completion = PairCompletion {
                    pair_id: job.pair_id,
                    lane_id,
                    previous: job.previous,
                    current: job.current,
                    result,
                    elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
                };
                if lane_completion_tx.send(completion).is_err() {
                    break;
                }
            }
        }));
    }
    drop(completion_tx);

    WorkerPool {
        job_senders,
        completion_rx,
        handles,
    }
}

fn join_workers(handles: Vec<thread::JoinHandle<()>>, run_result: Result<()>) -> Result<()> {
    let mut worker_error = None;
    for (lane_id, handle) in handles.into_iter().enumerate() {
        if handle.join().is_err() && worker_error.is_none() {
            worker_error = Some(anyhow!("parallel interpolator worker {lane_id} panicked"));
        }
    }

    match (run_result, worker_error) {
        (Err(error), _) => Err(error),
        (Ok(()), Some(error)) => Err(error),
        (Ok(()), None) => Ok(()),
    }
}
