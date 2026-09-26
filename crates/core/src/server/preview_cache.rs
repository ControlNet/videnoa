//! Preview cache ownership, bounded admission, and subprocess supervision.
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use cap_std::fs::Dir;
use tokio::io::AsyncReadExt;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

use super::AppError;

const MAX_SESSIONS: usize = 8;
const MAX_PROCESSED_FRAMES: usize = 8;
const MAX_SESSION_BYTES: u64 = 256 * 1024 * 1024;
const IDLE_TTL: Duration = Duration::from_secs(30 * 60);
pub(super) const EXTRACTION_TIMEOUT: Duration = Duration::from_secs(120);

struct CacheRoot {
    path: PathBuf,
    // Also held by active session leases, so a new server cannot sweep their files.
    _lock: File,
}

pub(super) struct PreviewSession {
    files: Dir,
    directory: tempfile::TempDir,
    _root: Arc<CacheRoot>,
    _slot: OwnedSemaphorePermit,
    last_used: Mutex<Instant>,
    results: Arc<Semaphore>,
}

impl PreviewSession {
    pub(super) fn path(&self) -> &Path {
        self.directory.path()
    }

    pub(super) fn touch(&self) {
        *self.last_used.lock().unwrap() = Instant::now();
    }

    pub(super) fn reserve_result(&self) -> Result<OwnedSemaphorePermit, AppError> {
        self.results.clone().try_acquire_owned().map_err(|_| {
            AppError::TooManyRequests(
                "preview result limit reached; extract frames again to start a new preview".into(),
            )
        })
    }

    pub(super) fn check_size(&self) -> Result<()> {
        let mut size = 0_u64;
        for entry in std::fs::read_dir(self.path())? {
            size = size.saturating_add(entry?.metadata()?.len());
            if size > MAX_SESSION_BYTES {
                bail!("preview exceeds the 256 MiB session limit");
            }
        }
        Ok(())
    }

    pub(super) fn read(&self, filename: &str) -> Result<Vec<u8>> {
        if !valid_filename(filename) {
            bail!("invalid preview filename");
        }
        // Capability-relative open prevents symlink traversal outside this directory,
        // including a symlink swapped between validation and opening.
        let mut file = self.files.open(filename)?;
        if !file.metadata()?.is_file() {
            bail!("preview is not a regular file");
        }
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(
            &mut std::io::Read::take(&mut file, MAX_SESSION_BYTES + 1),
            &mut bytes,
        )?;
        if bytes.len() as u64 > MAX_SESSION_BYTES {
            bail!("preview file exceeds size limit");
        }
        Ok(bytes)
    }
}

pub(super) fn valid_filename(filename: &str) -> bool {
    if let Some(number) = filename
        .strip_prefix("frame_")
        .and_then(|s| s.strip_suffix(".png"))
    {
        return number.len() == 4
            && number.bytes().all(|b| b.is_ascii_digit())
            && number.parse::<u32>().is_ok_and(|n| (1..=100).contains(&n));
    }
    filename
        .strip_prefix("processed-")
        .and_then(|s| s.strip_suffix(".png"))
        .is_some_and(|s| Uuid::parse_str(s).is_ok_and(|id| id.to_string() == s))
}

pub(super) struct PreviewCache {
    root: Arc<CacheRoot>,
    sessions: Mutex<HashMap<String, Arc<PreviewSession>>>,
    slots: Arc<Semaphore>,
    pub(super) extractions: Arc<Semaphore>,
    sweeping: AtomicBool,
}

impl PreviewCache {
    pub(super) fn open(data_dir: &Path) -> Result<Arc<Self>> {
        let path = data_dir.join("preview-cache");
        std::fs::create_dir_all(&path)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.join("cache.lock"))?;
        lock.try_lock()
            .context("preview cache is in use by another server")?;
        // Only our dedicated, locked cache is swept. Never touch global temporary
        // directories, which may belong to another Worker.
        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with("session-")
                && entry.file_type()?.is_dir()
            {
                std::fs::remove_dir_all(entry.path())?;
            }
        }
        let cache = Arc::new(Self {
            root: Arc::new(CacheRoot { path, _lock: lock }),
            sessions: Mutex::new(HashMap::new()),
            slots: Arc::new(Semaphore::new(MAX_SESSIONS)),
            extractions: Arc::new(Semaphore::new(2)),
            sweeping: AtomicBool::new(false),
        });
        cache.start_sweeper();
        Ok(cache)
    }

    fn start_sweeper(self: &Arc<Self>) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        if self.sweeping.swap(true, Ordering::Relaxed) {
            return;
        }
        let weak = Arc::downgrade(self);
        runtime.spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let Some(cache) = weak.upgrade() else {
                    break;
                };
                let _ = tokio::task::spawn_blocking(move || cache.sweep(Instant::now())).await;
            }
        });
    }

    fn sweep(&self, now: Instant) {
        self.sessions.lock().unwrap().retain(|_, session| {
            Arc::strong_count(session) > 1
                || now.saturating_duration_since(*session.last_used.lock().unwrap()) < IDLE_TTL
        });
    }

    pub(super) fn create(self: &Arc<Self>) -> Result<Arc<PreviewSession>, AppError> {
        self.start_sweeper();
        self.sweep(Instant::now());
        let slot = self.slots.clone().try_acquire_owned().map_err(|_| {
            AppError::TooManyRequests(
                "preview session limit reached; close an existing preview and retry".into(),
            )
        })?;
        let directory = tempfile::Builder::new()
            .prefix("session-")
            .tempdir_in(&self.root.path)
            .map_err(|e| AppError::Internal(format!("failed to create preview directory: {e}")))?;
        let files = Dir::open_ambient_dir(directory.path(), cap_std::ambient_authority())
            .map_err(|e| AppError::Internal(e.to_string()))?;
        Ok(Arc::new(PreviewSession {
            directory,
            files,
            _root: self.root.clone(),
            _slot: slot,
            last_used: Mutex::new(Instant::now()),
            results: Arc::new(Semaphore::new(MAX_PROCESSED_FRAMES)),
        }))
    }

    pub(super) fn insert(&self, id: String, session: Arc<PreviewSession>) {
        session.touch();
        self.sessions.lock().unwrap().insert(id, session);
    }

    pub(super) fn get(&self, id: &str) -> Result<Arc<PreviewSession>, AppError> {
        self.sweep(Instant::now());
        let sessions = self.sessions.lock().unwrap();
        let session = sessions.get(id).ok_or_else(|| {
            AppError::NotFound("preview session expired or not found; extract frames again".into())
        })?;
        session.touch();
        Ok(session.clone())
    }

    pub(super) fn release(&self, id: &str) {
        self.sessions.lock().unwrap().remove(id);
    }
}

/// Drain output concurrently, bound memory, reap on timeout/quota failure, and
/// kill on future cancellation. The caller holds extraction admission throughout.
pub(super) async fn run_command(
    command: std::process::Command,
    session: &PreviewSession,
    timeout: Duration,
) -> Result<Vec<u8>> {
    let mut command = tokio::process::Command::from(command);
    command
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command
        .spawn()
        .context("failed to launch preview subprocess")?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let result = tokio::time::timeout(timeout, async {
        let monitor = async {
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if let Err(error) = session.check_size() {
                    break error;
                }
            }
        };
        tokio::select! {
            outputs = async { tokio::try_join!(read_limited(stdout), read_limited(stderr), child.wait()) } => {
                let (stdout, stderr, status) = outputs?;
                if !status.success() { bail!("preview subprocess exited with {status}: {}", String::from_utf8_lossy(&stderr)); }
                session.check_size()?;
                Ok(stdout)
            }
            error = monitor => Err(error),
        }
    }).await.context("preview extraction timed out").and_then(|result| result);
    if result.is_err() {
        let _ = child.kill().await;
    }
    result
}

async fn read_limited(mut reader: impl tokio::io::AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let count = reader.read(&mut chunk).await?;
        if count == 0 {
            return Ok(output);
        }
        let keep = count.min((64 * 1024_usize).saturating_sub(output.len()));
        output.extend_from_slice(&chunk[..keep]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_exclude_paths_and_unpublished_forms() {
        for name in [
            "/tmp/frame_0001.png",
            "../frame_0001.png",
            "..\\frame_0001.png",
            "frame_0000.png",
            "frame_0101.png",
            "other.png",
        ] {
            assert!(!valid_filename(name), "{name}");
        }
        assert!(valid_filename("frame_0001.png"));
        assert!(valid_filename(&format!("processed-{}.png", Uuid::new_v4())));
    }

    #[test]
    fn release_and_expiry_respect_active_leases_and_clean_directories() {
        let dir = tempfile::tempdir().unwrap();
        let cache = PreviewCache::open(dir.path()).unwrap();
        let session = cache.create().unwrap();
        let path = session.path().to_owned();
        cache.insert("active".into(), session.clone());
        cache.sweep(Instant::now() + IDLE_TTL);
        assert!(cache.get("active").is_ok());
        cache.release("active");
        assert!(cache.get("active").is_err());
        assert!(path.exists());
        drop(session);
        assert!(!path.exists());
        let idle = cache.create().unwrap();
        let path = idle.path().to_owned();
        cache.insert("idle".into(), idle);
        cache.sweep(Instant::now() + IDLE_TTL);
        assert!(!path.exists());
    }

    #[test]
    fn admission_drop_and_restart_cleanup_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let cache = PreviewCache::open(dir.path()).unwrap();
        let sessions: Vec<_> = (0..MAX_SESSIONS).map(|_| cache.create().unwrap()).collect();
        assert!(cache.create().is_err());
        assert!(PreviewCache::open(dir.path()).is_err());
        let path = sessions[0].path().to_owned();
        drop(sessions);
        assert!(!path.exists());
        assert!(cache.create().is_ok());
        let orphan = cache.root.path.join("session-crash-test");
        std::fs::create_dir(&orphan).unwrap();
        drop(cache);
        let _reopened = PreviewCache::open(dir.path()).unwrap();
        assert!(!orphan.exists());
    }

    #[test]
    #[cfg(unix)]
    fn capability_open_rejects_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let cache = PreviewCache::open(dir.path()).unwrap();
        let session = cache.create().unwrap();
        let outside = dir.path().join("outside.txt");
        std::fs::write(&outside, "synthetic outside fixture").unwrap();
        std::os::unix::fs::symlink(&outside, session.path().join("frame_0001.png")).unwrap();
        assert!(session.read("frame_0001.png").is_err());
    }

    #[test]
    fn result_and_byte_quotas_release_failed_reservations() {
        let dir = tempfile::tempdir().unwrap();
        let cache = PreviewCache::open(dir.path()).unwrap();
        let session = cache.create().unwrap();
        let pending: Vec<_> = (0..MAX_PROCESSED_FRAMES)
            .map(|_| session.reserve_result().unwrap())
            .collect();
        assert!(session.reserve_result().is_err());
        drop(pending);
        for _ in 0..MAX_PROCESSED_FRAMES {
            session.reserve_result().unwrap().forget();
        }
        assert!(session.reserve_result().is_err());
        // A sparse synthetic fixture verifies the byte quota without allocating RAM.
        let file = File::create(session.path().join("frame_0001.png")).unwrap();
        file.set_len(MAX_SESSION_BYTES + 1).unwrap();
        assert!(session.check_size().is_err());
    }

    #[tokio::test]
    #[cfg(target_os = "linux")]
    async fn cancelled_subprocess_is_killed_and_failed_session_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let cache = PreviewCache::open(dir.path()).unwrap();
        let session = cache.create().unwrap();
        let path = session.path().to_owned();
        let pid_file = dir.path().join("child.pid");
        // Synthetic child exposes its PID, then becomes the supervised sleep process.
        let mut command = std::process::Command::new("sh");
        command
            .args(["-c", "echo $$ > \"$1\"; exec sleep 10", "preview-test"])
            .arg(&pid_file);
        let task =
            tokio::spawn(
                async move { run_command(command, &session, Duration::from_secs(5)).await },
            );
        let pid: u32 = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(text) = std::fs::read_to_string(&pid_file) {
                    if let Ok(pid) = text.trim().parse() {
                        break pid;
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::time::timeout(Duration::from_secs(2), async {
            while Path::new(&format!("/proc/{pid}")).exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("cancelled subprocess should be reaped");
        assert!(!path.exists());
        assert_eq!(cache.slots.available_permits(), MAX_SESSIONS);
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn subprocess_wait_yields_and_timeout_terminates_child() {
        let dir = tempfile::tempdir().unwrap();
        let cache = PreviewCache::open(dir.path()).unwrap();
        let session = cache.create().unwrap();
        let mut command = std::process::Command::new("sleep");
        command.arg("10");
        let start = Instant::now();
        let (result, timer) = tokio::join!(
            run_command(command, &session, Duration::from_millis(100)),
            async {
                tokio::time::sleep(Duration::from_millis(10)).await;
                start.elapsed()
            }
        );
        assert!(result.unwrap_err().to_string().contains("timed out"));
        assert!(timer < Duration::from_secs(1));
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
