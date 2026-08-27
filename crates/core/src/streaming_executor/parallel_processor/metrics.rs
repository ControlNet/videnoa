use std::time::{Duration, Instant};

pub(super) struct StageMetrics {
    started: Instant,
    pub(super) frames_processed: u64,
    pub(super) max_in_flight: usize,
    pub(super) worker_ms: f64,
    recv_ms: f64,
    send_ms: f64,
    send_attempts: u64,
}

impl StageMetrics {
    pub(super) fn new() -> Self {
        Self {
            started: Instant::now(),
            frames_processed: 0,
            max_in_flight: 0,
            worker_ms: 0.0,
            recv_ms: 0.0,
            send_ms: 0.0,
            send_attempts: 0,
        }
    }

    pub(super) fn record_receive(&mut self, elapsed: Duration) {
        self.recv_ms += elapsed.as_secs_f64() * 1000.0;
    }

    pub(super) fn record_send(&mut self, elapsed: Duration, _sent: bool) {
        self.send_ms += elapsed.as_secs_f64() * 1000.0;
        self.send_attempts = self.send_attempts.saturating_add(1);
    }

    fn average_send_wait_ms(&self) -> f64 {
        match self.send_attempts {
            0 => 0.0,
            attempts => self.send_ms / attempts as f64,
        }
    }

    pub(super) fn log(&self, stage_name: &str, lanes: usize) {
        if self.frames_processed == 0 {
            return;
        }
        tracing::info!(
            frames = self.frames_processed,
            lanes,
            max_in_flight = self.max_in_flight,
            avg_recv_wait_ms = format!("{:.1}", self.recv_ms / self.frames_processed as f64),
            avg_process_ms = format!("{:.1}", self.worker_ms / self.frames_processed as f64),
            avg_send_wait_ms = format!("{:.1}", self.average_send_wait_ms()),
            total_process_ms = format!("{:.0}", self.worker_ms),
            wall_stage_ms = format!("{:.0}", self.started.elapsed().as_secs_f64() * 1000.0),
            stage = stage_name,
            "Parallel processor stage summary"
        );
    }
}
