//! Helpers for the FFmpeg child processes Videnoa drives over pipes: a
//! bounded stderr capture for error messages, a deadline for `wait`, and a
//! deadline-bounded run with concurrent stdin/stdout/stderr.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStderr, Command, ExitStatus, Stdio};
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

    /// A tail with no reader, for tests whose child has no stderr pipe.
    #[cfg(test)]
    pub(crate) fn none() -> Self {
        Self { handle: None }
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

/// Result of [`output_with_timeout`].
pub(crate) struct TimedOutput {
    /// `None` when the deadline passed and the child was killed.
    pub(crate) status: Option<ExitStatus>,
    pub(crate) stdout: Vec<u8>,
    /// The last [`STDERR_TAIL_BYTES`] of stderr lines.
    pub(crate) stderr: String,
    /// Set when stdin could not be written completely (usually because the
    /// child exited early; its stderr then says why).
    pub(crate) stdin_error: Option<std::io::Error>,
    timeout: Duration,
}

impl TimedOutput {
    /// Fails unless the child exited successfully and took all of stdin.
    /// `what` names the process in the error.
    pub(crate) fn check(&self, what: &str) -> anyhow::Result<()> {
        let stderr = describe_stderr(&self.stderr);
        match self.status {
            None => anyhow::bail!(
                "{what} timed out after {}s and was killed; {stderr}",
                self.timeout.as_secs_f64()
            ),
            Some(status) if !status.success() => {
                anyhow::bail!("{what} exited with {status}; {stderr}")
            }
            Some(_) => match &self.stdin_error {
                Some(error) => anyhow::bail!("failed to write {what} input: {error}; {stderr}"),
                None => Ok(()),
            },
        }
    }
}

/// Runs `command` to completion within `timeout`, killing it on expiry.
///
/// `stdin` (if any) is written from its own thread while stdout and stderr
/// are drained concurrently, so a child that writes a lot before reading its
/// input cannot deadlock the pipes. stdout is kept in full and stderr as a
/// bounded tail.
pub(crate) fn output_with_timeout(
    command: &mut Command,
    stdin: Option<Vec<u8>>,
    timeout: Duration,
) -> std::io::Result<TimedOutput> {
    let mut child = command
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let writer = match (stdin, child.stdin.take()) {
        (Some(data), Some(mut pipe)) => Some(thread::spawn(move || {
            // Dropping the pipe afterwards signals EOF.
            pipe.write_all(&data)
        })),
        _ => None,
    };
    let mut stdout_pipe = child.stdout.take().expect("stdout is piped");
    let reader = thread::spawn(move || {
        let mut stdout = Vec::new();
        stdout_pipe.read_to_end(&mut stdout).map(|_| stdout)
    });
    let mut stderr = StderrTail::spawn(child.stderr.take().expect("stderr is piped"), |line| {
        tracing::debug!(target: "ffmpeg_subprocess_stderr", "{line}");
    });

    let status = wait_or_kill(&mut child, timeout)?;
    // The child has exited (or was killed), so every pipe is closed or
    // broken and the helper threads finish.
    let stdin_error = writer
        .and_then(|writer| writer.join().ok())
        .and_then(Result::err);
    let stdout = reader
        .join()
        .unwrap_or_else(|_| Ok(Vec::new()))
        .unwrap_or_default();
    Ok(TimedOutput {
        status,
        stdout,
        stderr: stderr.join(),
        stdin_error,
        timeout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[cfg(unix)]
    fn sh(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    #[cfg(unix)]
    fn output_with_timeout_feeds_stdin_and_collects_stdout_and_stderr() {
        let output = output_with_timeout(
            &mut sh("printf 'note\\n' >&2; tr a-z A-Z"),
            Some(b"pixels".to_vec()),
            Duration::from_secs(10),
        )
        .unwrap();
        assert!(output.status.unwrap().success());
        assert_eq!(output.stdout, b"PIXELS");
        assert_eq!(output.stderr, "note");
        assert!(output.stdin_error.is_none());
        output.check("tr").unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn output_with_timeout_reads_stderr_while_writing_stdin() {
        // The child fills its stderr pipe (64 KiB on Linux) before it reads
        // stdin; writing all of stdin before draining stderr would deadlock.
        let script = "i=0; while [ $i -lt 4000 ]; do \
                      printf 'warning line %06d padding padding\\n' $i >&2; i=$((i+1)); done; \
                      exec cat > /dev/null";
        let output = output_with_timeout(
            &mut sh(script),
            Some(vec![7_u8; 4 * 1024 * 1024]),
            Duration::from_secs(30),
        )
        .unwrap();
        assert!(output.status.unwrap().success());
        assert!(output.stdin_error.is_none());
        assert!(output
            .stderr
            .ends_with("warning line 003999 padding padding"));
    }

    #[test]
    #[cfg(unix)]
    fn output_with_timeout_kills_a_hung_child() {
        let start = Instant::now();
        let output = output_with_timeout(
            &mut sh("printf 'waiting for GPU\\n' >&2; exec sleep 30"),
            Some(vec![0_u8; 16]),
            Duration::from_millis(200),
        )
        .unwrap();
        assert!(start.elapsed() < Duration::from_secs(10));
        assert!(output.status.is_none());
        let message = output.check("preview decoder").unwrap_err().to_string();
        assert!(message.contains("preview decoder"), "{message}");
        assert!(message.contains("timed out after 0.2s"), "{message}");
        assert!(message.contains("waiting for GPU"), "{message}");
    }

    #[test]
    #[cfg(unix)]
    fn output_with_timeout_reports_failures_with_stderr() {
        let output = output_with_timeout(
            &mut sh("printf 'Invalid data found\\n' >&2; exit 4"),
            None,
            Duration::from_secs(10),
        )
        .unwrap();
        let message = output.check("ffprobe").unwrap_err().to_string();
        assert!(
            message.contains("ffprobe exited with exit status: 4"),
            "{message}"
        );
        assert!(message.contains("Invalid data found"), "{message}");
    }
}
