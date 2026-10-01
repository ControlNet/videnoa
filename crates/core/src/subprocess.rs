//! Helpers for the FFmpeg child processes Videnoa drives over pipes: a
//! bounded stderr capture for error messages and a deadline for `wait`.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStderr, ExitStatus};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Bytes of a child's most recent stderr lines kept for error messages.
pub(crate) const STDERR_TAIL_BYTES: usize = 4096;

/// Drains a child's stderr on a background thread so the child never blocks
/// on a full pipe, forwards each line to `log`, and keeps the last
/// [`STDERR_TAIL_BYTES`] of lines so a failure can quote FFmpeg's own message.
pub(crate) struct StderrTail {
    handle: Option<JoinHandle<String>>,
}

impl StderrTail {
    pub(crate) fn spawn(stderr: ChildStderr, log: impl Fn(&str) + Send + 'static) -> Self {
        let handle = thread::spawn(move || {
            let mut tail: VecDeque<String> = VecDeque::new();
            let mut tail_bytes = 0_usize;
            for line in BufReader::new(stderr).lines() {
                let line = match line {
                    Ok(line) => line,
                    Err(error) => {
                        log(&format!("read error: {error}"));
                        break;
                    }
                };
                if line.is_empty() {
                    continue;
                }
                log(&line);
                // Count the newline that joins lines back together.
                tail_bytes += line.len() + 1;
                tail.push_back(line);
                while tail_bytes > STDERR_TAIL_BYTES && tail.len() > 1 {
                    tail_bytes -= tail.pop_front().map_or(0, |dropped| dropped.len() + 1);
                }
            }
            tail.into_iter().collect::<Vec<_>>().join("\n")
        });
        Self {
            handle: Some(handle),
        }
    }

    /// Waits for the child's stderr to close (the child has exited or closed
    /// it) and returns the retained tail. Empty once already joined.
    pub(crate) fn join(&mut self) -> String {
        self.handle
            .take()
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default()
    }
}

/// `tail` formatted for appending to an error message.
pub(crate) fn describe_stderr(tail: &str) -> String {
    let tail = tail.trim();
    if tail.is_empty() {
        "(no stderr output)".to_string()
    } else {
        format!("stderr: {tail}")
    }
}

/// Waits for `child` to exit. When `timeout` elapses first, the child is
/// killed and reaped and `Ok(None)` is returned.
pub(crate) fn wait_or_kill(
    child: &mut Child,
    timeout: Duration,
) -> std::io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex};

    #[cfg(unix)]
    fn spawn_sh(script: &str) -> Child {
        Command::new("sh")
            .args(["-c", script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    #[test]
    #[cfg(unix)]
    fn stderr_tail_logs_every_line_and_keeps_the_last_bytes() {
        // 300 lines of 20 bytes exceed the 4 KiB tail; only the newest survive.
        let mut child = spawn_sh(
            "i=0; while [ $i -lt 300 ]; do printf 'line-%014d\\n' $i >&2; i=$((i+1)); done",
        );
        let logged = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&logged);
        let mut tail = StderrTail::spawn(child.stderr.take().unwrap(), move |line| {
            sink.lock().unwrap().push(line.to_string());
        });
        child.wait().unwrap();

        let tail = tail.join();
        assert_eq!(logged.lock().unwrap().len(), 300);
        assert!(tail.len() <= STDERR_TAIL_BYTES, "{}", tail.len());
        assert!(tail.ends_with("line-00000000000299"), "{tail}");
        assert!(!tail.contains("line-00000000000000"), "{tail}");
        assert_eq!(describe_stderr(&tail), format!("stderr: {tail}"));
        assert_eq!(describe_stderr(" \n"), "(no stderr output)");
    }

    #[test]
    #[cfg(unix)]
    fn wait_or_kill_returns_the_status_of_a_finished_child() {
        let mut child = spawn_sh("exit 3");
        let status = wait_or_kill(&mut child, Duration::from_secs(5))
            .unwrap()
            .expect("child should exit before the deadline");
        assert_eq!(status.code(), Some(3));
    }

    #[test]
    #[cfg(unix)]
    fn wait_or_kill_kills_a_child_that_outlives_the_deadline() {
        let mut child = spawn_sh("sleep 30");
        let start = Instant::now();
        let status = wait_or_kill(&mut child, Duration::from_millis(100)).unwrap();
        assert!(status.is_none());
        assert!(start.elapsed() < Duration::from_secs(5));
        // The child was reaped: a second wait reports the kill, not a hang.
        assert!(!child.wait().unwrap().success());
    }
}
