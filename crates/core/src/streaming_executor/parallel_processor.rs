use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc as std_mpsc, Arc};
use std::thread;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use tokio::sync::mpsc;

use super::IndexedFrame;
use crate::node::{ExecutionContext, FrameProcessor};
use coordinator::{CoordinatorChannels, CoordinatorControl, FrameCoordinator};

mod coordinator;
mod metrics;

pub(super) struct ParallelProcessorRun {
    pub(super) processors: Vec<Box<dyn FrameProcessor>>,
    pub(super) input: mpsc::Receiver<IndexedFrame>,
    pub(super) output: mpsc::Sender<IndexedFrame>,
    pub(super) total_frames: Option<u64>,
    pub(super) cancel_state: Arc<AtomicBool>,
    pub(super) stage_name: String,
}

pub(super) fn run_parallel_processor_loop(run: ParallelProcessorRun) -> Result<()> {
    let ParallelProcessorRun {
        processors,
        mut input,
        output,
        total_frames,
        cancel_state,
        stage_name,
    } = run;
    if processors.is_empty() {
        bail!("parallel processor requires at least one lane");
    }

    let WorkerPool {
        job_senders,
        completion_rx,
        handles,
    } = spawn_workers(processors, total_frames, &stage_name)?;
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
    let run_result = FrameCoordinator::new(channels, control).run();

    drop(job_senders);
    join_workers(handles, run_result)
}

struct ProcessorJob {
    job_id: u64,
    frame: IndexedFrame,
}

struct ProcessorCompletion {
    job_id: u64,
    lane_id: usize,
    result: Result<IndexedFrame>,
    elapsed_ms: f64,
}

struct WorkerPool {
    job_senders: Vec<std_mpsc::SyncSender<ProcessorJob>>,
    completion_rx: std_mpsc::Receiver<ProcessorCompletion>,
    handles: Vec<thread::JoinHandle<()>>,
}

fn spawn_workers(
    processors: Vec<Box<dyn FrameProcessor>>,
    total_frames: Option<u64>,
    stage_name: &str,
) -> Result<WorkerPool> {
    let (completion_tx, completion_rx) = std_mpsc::channel();
    let worker_count = processors.len();
    let mut job_senders = Vec::new();
    job_senders
        .try_reserve_exact(worker_count)
        .context("failed to reserve parallel processor job senders")?;
    let mut handles = Vec::new();
    handles
        .try_reserve_exact(worker_count)
        .context("failed to reserve parallel processor worker handles")?;

    for (lane_id, mut processor) in processors.into_iter().enumerate() {
        let (job_tx, job_rx) = std_mpsc::sync_channel::<ProcessorJob>(1);
        let lane_completion_tx = completion_tx.clone();
        let lane_stage_name = stage_name.to_string();
        let worker = thread::Builder::new()
            .name(format!("videnoa-processor-worker-{lane_id}"))
            .spawn(move || {
                while let Ok(job) = job_rx.recv() {
                    let frame_index = job.frame.index;
                    let IndexedFrame {
                        index,
                        timestamp,
                        frame,
                        is_scene_change,
                    } = job.frame;
                    let ctx = ExecutionContext {
                        total_frames,
                        current_frame: frame_index,
                        ..Default::default()
                    };
                    let started = Instant::now();
                    let result = match catch_unwind(AssertUnwindSafe(|| {
                        processor.process_frame(frame, &ctx)
                    })) {
                        Ok(result) => result
                            .with_context(|| {
                                format!(
                                    "processor '{lane_stage_name}' failed on frame {frame_index}"
                                )
                            })
                            .map(|frame| IndexedFrame {
                                index,
                                timestamp,
                                frame,
                                is_scene_change,
                            }),
                        Err(_) => Err(anyhow!(
                            "processor '{lane_stage_name}' panicked on frame {frame_index}"
                        )),
                    };
                    let completion = ProcessorCompletion {
                        job_id: job.job_id,
                        lane_id,
                        result,
                        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
                    };
                    if lane_completion_tx.send(completion).is_err() {
                        break;
                    }
                }
            });
        match worker {
            Ok(handle) => {
                job_senders.push(job_tx);
                handles.push(handle);
            }
            Err(error) => {
                drop(job_tx);
                drop(job_senders);
                for handle in handles {
                    let _ = handle.join();
                }
                return Err(error).context(format!(
                    "failed to spawn parallel processor worker {lane_id}"
                ));
            }
        }
    }
    drop(completion_tx);

    Ok(WorkerPool {
        job_senders,
        completion_rx,
        handles,
    })
}

fn join_workers(handles: Vec<thread::JoinHandle<()>>, run_result: Result<()>) -> Result<()> {
    let mut worker_error = None;
    for (lane_id, handle) in handles.into_iter().enumerate() {
        if handle.join().is_err() && worker_error.is_none() {
            worker_error = Some(anyhow!("parallel processor worker {lane_id} panicked"));
        }
    }

    match (run_result, worker_error) {
        (Err(error), _) => Err(error),
        (Ok(()), Some(error)) => Err(error),
        (Ok(()), None) => Ok(()),
    }
}
