use std::time::{Duration, Instant};

pub(super) struct StageMetrics {
    started: Instant,
    pub(super) pairs_processed: u64,
    pub(super) max_in_flight: usize,
    pub(super) worker_ms: f64,
    pub(super) recv_ms: f64,
    pub(super) send_ms: f64,
    output_frames: u64,
    send_attempts: u64,
}

impl StageMetrics {
    pub(super) fn new() -> Self {
        Self {
            started: Instant::now(),
            pairs_processed: 0,
            max_in_flight: 0,
            worker_ms: 0.0,
            recv_ms: 0.0,
            send_ms: 0.0,
            output_frames: 0,
            send_attempts: 0,
        }
    }

    pub(super) fn record_receive(&mut self, elapsed: Duration) {
        self.recv_ms += elapsed.as_secs_f64() * 1000.0;
    }

    pub(super) fn record_send(&mut self, elapsed: Duration, sent: bool) {
        self.send_ms += elapsed.as_secs_f64() * 1000.0;
        self.send_attempts = self.send_attempts.saturating_add(1);
        if sent {
            self.output_frames = self.output_frames.saturating_add(1);
        }
    }

    pub(super) fn output_frames(&self) -> u64 {
        self.output_frames
    }

    fn average_send_wait_ms(&self) -> f64 {
        match self.send_attempts {
            0 => 0.0,
            attempts => self.send_ms / attempts as f64,
        }
    }

    pub(super) fn log(&self, stage_name: &str, lanes: usize) {
        if self.pairs_processed == 0 {
            return;
        }
        tracing::info!(
            pairs = self.pairs_processed,
            output_frames = self.output_frames,
            lanes,
            max_in_flight = self.max_in_flight,
            avg_recv_wait_ms = format!("{:.1}", self.recv_ms / self.pairs_processed as f64),
            avg_interpolate_ms = format!("{:.1}", self.worker_ms / self.pairs_processed as f64),
            avg_send_wait_ms = format!("{:.1}", self.average_send_wait_ms()),
            total_interpolate_ms = format!("{:.0}", self.worker_ms),
            wall_stage_ms = format!("{:.0}", self.started.elapsed().as_secs_f64() * 1000.0),
            stage = stage_name,
            "Parallel interpolator stage summary"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_frames_when_no_send_succeeds_is_zero() {
        // Given
        let metrics = StageMetrics::new();

        // Then
        assert_eq!(metrics.output_frames(), 0);
    }

    #[test]
    fn output_frames_when_one_send_succeeds_is_one() {
        // Given
        let mut metrics = StageMetrics::new();

        // When
        metrics.record_send(Duration::ZERO, true);

        // Then
        assert_eq!(metrics.output_frames(), 1);
    }

    #[test]
    fn output_frames_when_multiple_sends_succeed_excludes_failed_send() {
        // Given
        let mut metrics = StageMetrics::new();

        // When
        for sent in [true, true, false, true] {
            metrics.record_send(Duration::ZERO, sent);
        }

        // Then
        assert_eq!(metrics.output_frames(), 3);
    }

    #[test]
    fn average_send_wait_when_one_attempt_fails_uses_all_attempts() {
        // Given
        let mut metrics = StageMetrics::new();

        // When
        metrics.record_send(Duration::from_millis(10), true);
        metrics.record_send(Duration::from_millis(30), false);

        // Then
        assert_eq!(metrics.output_frames(), 1);
        assert!((metrics.average_send_wait_ms() - 20.0).abs() < f64::EPSILON);
    }
}
