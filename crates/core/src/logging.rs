use std::{
    any::Any,
    backtrace::{Backtrace, BacktraceStatus},
    fs,
    io::{self, Write},
    panic::{self, PanicHookInfo},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    thread,
};

use tracing::Metadata;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt::writer::MakeWriter;

pub const DEFAULT_LOG_FILTER: &str = "info";
pub const DEFAULT_NOISE_FILTER: &str =
    "ort=error,ffmpeg_stderr=error,ffmpeg_encode_stderr=error,ffmpeg_stream_stderr=error";
pub const DEFAULT_LOG_RETENTION_FILES: usize = 14;
pub const DEFAULT_LOG_DIR_NAME: &str = "logs";
pub const DEFAULT_CRASH_DIR_NAME: &str = "crash";
pub const DEFAULT_LOG_FILE_PREFIX: &str = "videnoa";
pub const DEFAULT_LOG_FILE_SUFFIX: &str = "log";
pub const REDACTION_PLACEHOLDER: &str = "***REDACTED***";

const FFMPEG_DEBUG_TARGETS: [&str; 3] = [
    "ffmpeg_stderr",
    "ffmpeg_encode_stderr",
    "ffmpeg_stream_stderr",
];

static PANIC_HOOK_INSTALL_LOCK: Mutex<()> = Mutex::new(());
static PANIC_HOOK_CRASH_DIR: OnceLock<PathBuf> = OnceLock::new();
static PANIC_HOOK_WRITE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static PANIC_ARTIFACT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLogMode {
    Cli,
    Server,
    Desktop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggingInitOptions {
    pub mode: RuntimeLogMode,
    pub data_dir: Option<PathBuf>,
    pub verbose: u8,
    pub cli_log_filter: Option<String>,
    pub rust_log_env: Option<String>,
    pub default_log_filter: String,
    pub noise_filter: String,
    pub include_noise_filter_when_implicit: bool,
    pub retention_files: usize,
}

impl Default for LoggingInitOptions {
    fn default() -> Self {
        Self {
            mode: RuntimeLogMode::Server,
            data_dir: None,
            verbose: 0,
            cli_log_filter: None,
            rust_log_env: None,
            default_log_filter: DEFAULT_LOG_FILTER.to_string(),
            noise_filter: DEFAULT_NOISE_FILTER.to_string(),
            include_noise_filter_when_implicit: true,
            retention_files: DEFAULT_LOG_RETENTION_FILES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggingFilterPlan {
    pub user_filter: String,
    pub console_filter: String,
    pub file_filter: String,
}

#[derive(Debug)]
pub struct LoggingInitPlan {
    pub filters: LoggingFilterPlan,
    pub file_sink: FileSinkPlan,
}

#[derive(Debug)]
pub enum FileSinkPlan {
    Ready(ReadyFileSinkPlan),
    Fallback(FallbackFileSinkPlan),
}

#[derive(Debug)]
pub struct ReadyFileSinkPlan {
    pub log_dir: PathBuf,
    pub retention_files: usize,
    pub appender: RollingFileAppender,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackFileSinkPlan {
    pub attempted_log_dir: Option<PathBuf>,
    pub retention_files: usize,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanicHookInstallPlan {
    Installed {
        crash_dir: PathBuf,
    },
    AlreadyInstalled {
        crash_dir: PathBuf,
    },
    Fallback {
        attempted_crash_dir: Option<PathBuf>,
        reason: String,
    },
}

#[derive(Debug)]
struct PanicArtifactRecord {
    timestamp: chrono::DateTime<chrono::Utc>,
    thread_name: String,
    source_location: String,
    payload: String,
    backtrace_policy: String,
    backtrace_text: String,
}

#[derive(Debug)]
pub struct RedactingMakeWriter<M> {
    inner: M,
}

#[derive(Debug)]
pub struct RedactingWriter<W: Write> {
    inner: W,
    pending: Vec<u8>,
}

impl FileSinkPlan {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }

    pub fn retention_files(&self) -> usize {
        match self {
            Self::Ready(plan) => plan.retention_files,
            Self::Fallback(plan) => plan.retention_files,
        }
    }

    pub fn log_dir(&self) -> Option<&PathBuf> {
        match self {
            Self::Ready(plan) => Some(&plan.log_dir),
            Self::Fallback(plan) => plan.attempted_log_dir.as_ref(),
        }
    }

    pub fn fallback_reason(&self) -> Option<&str> {
        match self {
            Self::Ready(_) => None,
            Self::Fallback(plan) => Some(plan.reason.as_str()),
        }
    }
}

pub fn redacting_make_writer<M>(inner: M) -> RedactingMakeWriter<M> {
    RedactingMakeWriter { inner }
}

impl<M> RedactingMakeWriter<M> {
    pub fn new(inner: M) -> Self {
        Self { inner }
    }
}

impl<W: Write> RedactingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            pending: Vec::new(),
        }
    }
}

impl<'a, M> MakeWriter<'a> for RedactingMakeWriter<M>
where
    M: MakeWriter<'a>,
{
    type Writer = RedactingWriter<M::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter::new(self.inner.make_writer())
    }

    fn make_writer_for(&'a self, metadata: &Metadata<'_>) -> Self::Writer {
        RedactingWriter::new(self.inner.make_writer_for(metadata))
    }
}

impl<W: Write> RedactingWriter<W> {
    fn flush_complete_lines(&mut self) -> io::Result<()> {
        while let Some(newline_index) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=newline_index).collect();
            self.write_redacted_bytes(&line)?;
        }
        Ok(())
    }

    fn flush_all_pending(&mut self) -> io::Result<()> {
        if !self.pending.is_empty() {
            let chunk: Vec<u8> = self.pending.drain(..).collect();
            self.write_redacted_bytes(&chunk)?;
        }
        Ok(())
    }

    fn write_redacted_bytes(&mut self, chunk: &[u8]) -> io::Result<()> {
        let text = String::from_utf8_lossy(chunk);
        let redacted = redact_sensitive_text(text.as_ref());
        self.inner.write_all(redacted.as_bytes())
    }
}

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buf);
        self.flush_complete_lines()?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_all_pending()?;
        self.inner.flush()
    }
}

impl<W: Write> Drop for RedactingWriter<W> {
    fn drop(&mut self) {
        let _ = self.flush_all_pending();
        let _ = self.inner.flush();
    }
}

pub fn compose_logging_init_plan(options: &LoggingInitOptions) -> LoggingInitPlan {
    LoggingInitPlan {
        filters: compose_logging_filters(options),
        file_sink: build_file_sink_plan(options),
    }
}

pub fn install_panic_hook(data_dir: Option<&Path>) -> PanicHookInstallPlan {
    if let Some(existing_crash_dir) = PANIC_HOOK_CRASH_DIR.get() {
        return PanicHookInstallPlan::AlreadyInstalled {
            crash_dir: existing_crash_dir.clone(),
        };
    }

    let Some(data_dir) = data_dir else {
        return PanicHookInstallPlan::Fallback {
            attempted_crash_dir: None,
            reason: "panic hook disabled: data_dir is not configured".to_string(),
        };
    };

    let crash_dir = data_dir
        .join(DEFAULT_LOG_DIR_NAME)
        .join(DEFAULT_CRASH_DIR_NAME);
    if let Err(error) = fs::create_dir_all(&crash_dir) {
        return PanicHookInstallPlan::Fallback {
            attempted_crash_dir: Some(crash_dir),
            reason: format!("failed to create crash artifact directory: {error}"),
        };
    }

    let _install_guard = PANIC_HOOK_INSTALL_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if let Some(existing_crash_dir) = PANIC_HOOK_CRASH_DIR.get() {
        return PanicHookInstallPlan::AlreadyInstalled {
            crash_dir: existing_crash_dir.clone(),
        };
    }

    let previous_hook = panic::take_hook();
    let crash_dir_for_hook = crash_dir.clone();
    panic::set_hook(Box::new(move |panic_info| {
        write_panic_artifact_with_fallback(&crash_dir_for_hook, panic_info);
        previous_hook(panic_info);
    }));

    let _ = PANIC_HOOK_CRASH_DIR.set(crash_dir.clone());
    PanicHookInstallPlan::Installed { crash_dir }
}

pub fn build_file_sink_plan(options: &LoggingInitOptions) -> FileSinkPlan {
    let retention_files = normalize_retention_files(options.retention_files);

    let Some(data_dir) = options.data_dir.as_deref() else {
        return FileSinkPlan::Fallback(FallbackFileSinkPlan {
            attempted_log_dir: None,
            retention_files,
            reason: "file sink disabled: data_dir is not configured".to_string(),
        });
    };

    let log_dir = data_dir.join(DEFAULT_LOG_DIR_NAME);
    if let Err(error) = fs::create_dir_all(&log_dir) {
        return FileSinkPlan::Fallback(FallbackFileSinkPlan {
            attempted_log_dir: Some(log_dir),
            retention_files,
            reason: format!("failed to create log directory: {error}"),
        });
    }

    let appender_builder = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(DEFAULT_LOG_FILE_PREFIX)
        .filename_suffix(DEFAULT_LOG_FILE_SUFFIX)
        .max_log_files(retention_files);

    match appender_builder.build(&log_dir) {
        Ok(appender) => FileSinkPlan::Ready(ReadyFileSinkPlan {
            log_dir,
            retention_files,
            appender,
        }),
        Err(error) => FileSinkPlan::Fallback(FallbackFileSinkPlan {
            attempted_log_dir: Some(log_dir),
            retention_files,
            reason: format!("failed to initialize rolling file sink: {error}"),
        }),
    }
}

pub fn compose_logging_filters(options: &LoggingInitOptions) -> LoggingFilterPlan {
    let user_filter = select_user_filter(options);
    let should_include_noise = options.include_noise_filter_when_implicit
        && options.cli_log_filter.is_none()
        && options.verbose == 0;

    let console_filter = merge_noise_filter(
        options.noise_filter.as_str(),
        user_filter.as_str(),
        should_include_noise,
    );
    let file_filter = if should_include_noise {
        let file_noise_filter = rewrite_noise_filter_for_file(options.noise_filter.as_str());
        merge_noise_filter(file_noise_filter.as_str(), user_filter.as_str(), true)
    } else {
        user_filter.clone()
    };

    LoggingFilterPlan {
        user_filter,
        file_filter,
        console_filter,
    }
}

pub fn select_log_filter(options: &LoggingInitOptions) -> String {
    compose_logging_filters(options).console_filter
}

fn normalize_retention_files(retention_files: usize) -> usize {
    if retention_files == 0 {
        DEFAULT_LOG_RETENTION_FILES
    } else {
        retention_files
    }
}

fn select_user_filter(options: &LoggingInitOptions) -> String {
    if let Some(filter) = options.cli_log_filter.as_deref() {
        filter.to_string()
    } else if options.verbose >= 2 {
        "trace".to_string()
    } else if options.verbose == 1 {
        "debug".to_string()
    } else if let Some(filter) = options.rust_log_env.as_deref() {
        filter.to_string()
    } else {
        options.default_log_filter.clone()
    }
}

fn merge_noise_filter(noise_filter: &str, user_filter: &str, include_noise_filter: bool) -> String {
    if include_noise_filter && !noise_filter.trim().is_empty() {
        format!("{noise_filter},{user_filter}")
    } else {
        user_filter.to_string()
    }
}

fn rewrite_noise_filter_for_file(noise_filter: &str) -> String {
    let mut rewritten_directives = Vec::new();
    let mut ffmpeg_targets_seen: Vec<&str> = Vec::new();

    for directive in noise_filter
        .split(',')
        .map(str::trim)
        .filter(|directive| !directive.is_empty())
    {
        if let Some((target, _)) = directive.split_once('=') {
            let target = target.trim();
            if is_ffmpeg_target(target) {
                if !ffmpeg_targets_seen.contains(&target) {
                    rewritten_directives.push(format!("{target}=debug"));
                    ffmpeg_targets_seen.push(target);
                }
                continue;
            }
        }

        rewritten_directives.push(directive.to_string());
    }

    for target in FFMPEG_DEBUG_TARGETS {
        if !ffmpeg_targets_seen.contains(&target) {
            rewritten_directives.push(format!("{target}=debug"));
        }
    }

    rewritten_directives.join(",")
}

fn is_ffmpeg_target(target: &str) -> bool {
    FFMPEG_DEBUG_TARGETS.contains(&target)
}

pub fn redact_sensitive_text(input: &str) -> String {
    let with_redacted_userinfo = redact_url_credentials(input);
    redact_sensitive_assignments(with_redacted_userinfo.as_str())
}

fn redact_url_credentials(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;

    while let Some(scheme_offset) = input[cursor..].find("://") {
        let scheme_separator = cursor + scheme_offset;
        let authority_start = scheme_separator + 3;
        let authority_end = input[authority_start..]
            .find(|ch: char| {
                matches!(
                    ch,
                    '/' | '?' | '#' | ' ' | '\t' | '\r' | '\n' | '"' | '\'' | '<' | '>'
                )
            })
            .map(|offset| authority_start + offset)
            .unwrap_or(input.len());

        if let Some(userinfo_offset) = input[authority_start..authority_end].rfind('@') {
            let userinfo_end = authority_start + userinfo_offset;
            if userinfo_end > authority_start {
                output.push_str(&input[cursor..authority_start]);
                output.push_str(REDACTION_PLACEHOLDER);
                output.push_str(&input[userinfo_end..authority_end]);
                cursor = authority_end;
                continue;
            }
        }

        output.push_str(&input[cursor..authority_end]);
        cursor = authority_end;
    }

    output.push_str(&input[cursor..]);
    output
}

fn redact_sensitive_assignments(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0usize;
    let mut index = 0usize;

    while index < bytes.len() {
        let separator = bytes[index];
        if separator == b'=' || separator == b':' {
            let mut key_start = index;
            while key_start > 0 {
                let previous = bytes[key_start - 1];
                if previous.is_ascii_alphanumeric() || previous == b'_' || previous == b'-' {
                    key_start -= 1;
                } else {
                    break;
                }
            }

            if key_start < index {
                let key = input[key_start..index].to_ascii_lowercase();
                if is_sensitive_key(key.as_str()) {
                    let mut value_start = index + 1;
                    while value_start < bytes.len() && bytes[value_start].is_ascii_whitespace() {
                        value_start += 1;
                    }

                    if value_start < bytes.len() {
                        let mut redact_start = value_start;
                        if input[value_start..]
                            .get(..7)
                            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bearer "))
                        {
                            redact_start = value_start + 7;
                        }

                        if redact_start >= bytes.len() {
                            index += 1;
                            continue;
                        }

                        let (prefix_end, value_end) =
                            if bytes[redact_start] == b'\'' || bytes[redact_start] == b'"' {
                                let quote = bytes[redact_start];
                                let value_content_start = redact_start + 1;
                                let quote_end = input[value_content_start..]
                                    .find(quote as char)
                                    .map(|offset| value_content_start + offset)
                                    .unwrap_or(input.len());
                                (value_content_start, quote_end)
                            } else {
                                (
                                    redact_start,
                                    find_unquoted_value_end(input, bytes, redact_start),
                                )
                            };

                        if value_end > prefix_end {
                            output.push_str(&input[cursor..prefix_end]);
                            output.push_str(REDACTION_PLACEHOLDER);
                            cursor = value_end;
                            index = value_end;
                            continue;
                        }
                    }
                }
            }
        }

        index += 1;
    }

    output.push_str(&input[cursor..]);
    output
}

fn find_unquoted_value_end(input: &str, bytes: &[u8], start: usize) -> usize {
    let mut index = start;
    while index < bytes.len() {
        let current = bytes[index];
        if current.is_ascii_whitespace()
            || matches!(
                current,
                b'&' | b',' | b';' | b')' | b']' | b'}' | b'"' | b'\''
            )
        {
            break;
        }
        index += 1;
    }

    if index == start {
        input.len()
    } else {
        index
    }
}

fn is_sensitive_key(key: &str) -> bool {
    if key == "key" || key == "pwd" || key == "passwd" || key == "authorization" {
        return true;
    }

    if key.contains("token") || key.contains("secret") || key.contains("password") {
        return true;
    }

    key.ends_with("_key")
        || key.ends_with("-key")
        || key.ends_with("api_key")
        || key.ends_with("api-key")
        || key.ends_with("apikey")
}

fn write_panic_artifact_with_fallback(crash_dir: &Path, panic_info: &PanicHookInfo<'_>) {
    if PANIC_HOOK_WRITE_IN_PROGRESS
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    let write_result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        let report = build_panic_artifact_record(panic_info);
        write_panic_artifact_file(crash_dir, &report)
    }));

    match write_result {
        Ok(Ok(_artifact_path)) => {}
        Ok(Err(error)) => {
            eprintln!(
                "Warning: failed to write panic crash artifact under '{}': {error}",
                crash_dir.display()
            );
        }
        Err(_) => {
            eprintln!(
                "Warning: panic hook failed while writing crash artifact under '{}'.",
                crash_dir.display()
            );
        }
    }

    PANIC_HOOK_WRITE_IN_PROGRESS.store(false, Ordering::Release);
}

fn build_panic_artifact_record(panic_info: &PanicHookInfo<'_>) -> PanicArtifactRecord {
    let (backtrace_policy, backtrace_text) = capture_backtrace_details();

    let source_location = panic_info
        .location()
        .map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        })
        .unwrap_or_else(|| "<unknown>".to_string());

    PanicArtifactRecord {
        timestamp: chrono::Utc::now(),
        thread_name: thread::current().name().unwrap_or("<unnamed>").to_string(),
        source_location,
        payload: panic_payload_to_string(panic_info.payload()),
        backtrace_policy,
        backtrace_text,
    }
}

fn capture_backtrace_details() -> (String, String) {
    let backtrace = Backtrace::capture();
    match backtrace.status() {
        BacktraceStatus::Captured => ("captured".to_string(), backtrace.to_string()),
        BacktraceStatus::Disabled => (
            "disabled (set RUST_BACKTRACE=1/full to enable)".to_string(),
            "<disabled by backtrace policy>".to_string(),
        ),
        BacktraceStatus::Unsupported => (
            "unsupported".to_string(),
            "<backtrace unsupported on this platform>".to_string(),
        ),
        _ => (
            "unknown".to_string(),
            "<backtrace status unknown>".to_string(),
        ),
    }
}

fn write_panic_artifact_file(
    crash_dir: &Path,
    report: &PanicArtifactRecord,
) -> std::io::Result<PathBuf> {
    fs::create_dir_all(crash_dir)?;

    let sequence = PANIC_ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let filename = format!(
        "panic-{}-{sequence:06}.log",
        report.timestamp.format("%Y%m%d-%H%M%S-%f")
    );
    let artifact_path = crash_dir.join(filename);

    let mut file = fs::File::create(&artifact_path)?;
    writeln!(file, "timestamp_utc={}", report.timestamp.to_rfc3339())?;
    writeln!(file, "thread={}", report.thread_name)?;
    writeln!(file, "location={}", report.source_location)?;
    writeln!(file, "payload={}", report.payload)?;
    writeln!(file, "backtrace_policy={}", report.backtrace_policy)?;
    writeln!(file, "backtrace:")?;
    writeln!(file, "{}", report.backtrace_text)?;
    file.flush()?;

    Ok(artifact_path)
}

fn panic_payload_to_string(payload: &(dyn Any + Send)) -> String {
    if let Some(payload) = payload.downcast_ref::<&str>() {
        (*payload).to_string()
    } else if let Some(payload) = payload.downcast_ref::<String>() {
        payload.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;
    use tempfile::{tempdir, NamedTempFile};

    #[test]
    fn cli_log_filter_overrides_everything() {
        let options = LoggingInitOptions {
            verbose: 2,
            cli_log_filter: Some("videnoa_core=trace".to_string()),
            rust_log_env: Some("error".to_string()),
            ..Default::default()
        };

        let filters = compose_logging_filters(&options);
        assert_eq!(filters.user_filter, "videnoa_core=trace");
        assert_eq!(filters.console_filter, "videnoa_core=trace");
    }

    #[test]
    fn verbose_two_maps_to_trace() {
        let options = LoggingInitOptions {
            verbose: 2,
            rust_log_env: Some("warn".to_string()),
            ..Default::default()
        };

        let filters = compose_logging_filters(&options);
        assert_eq!(filters.user_filter, "trace");
        assert_eq!(filters.console_filter, "trace");
    }

    #[test]
    fn verbose_one_maps_to_debug() {
        let options = LoggingInitOptions {
            verbose: 1,
            rust_log_env: Some("warn".to_string()),
            ..Default::default()
        };

        let filters = compose_logging_filters(&options);
        assert_eq!(filters.user_filter, "debug");
        assert_eq!(filters.console_filter, "debug");
    }

    #[test]
    fn rust_log_env_used_when_no_cli_or_verbose() {
        let options = LoggingInitOptions {
            rust_log_env: Some("warn,my_crate=debug".to_string()),
            ..Default::default()
        };

        let filters = compose_logging_filters(&options);
        assert_eq!(filters.user_filter, "warn,my_crate=debug");
    }

    #[test]
    fn noise_filter_included_for_implicit_filter_selection() {
        let options = LoggingInitOptions {
            rust_log_env: Some("info".to_string()),
            ..Default::default()
        };

        let filters = compose_logging_filters(&options);
        assert_eq!(
            filters.console_filter,
            format!("{DEFAULT_NOISE_FILTER},info")
        );
        assert_eq!(
            filters.file_filter,
            "ort=error,ffmpeg_stderr=debug,ffmpeg_encode_stderr=debug,ffmpeg_stream_stderr=debug,info"
        );
    }

    #[test]
    fn noise_filter_not_included_for_explicit_filter_selection() {
        let explicit_cli = LoggingInitOptions {
            cli_log_filter: Some("trace".to_string()),
            ..Default::default()
        };
        let explicit_verbose = LoggingInitOptions {
            verbose: 1,
            ..Default::default()
        };

        assert_eq!(
            compose_logging_filters(&explicit_cli).console_filter,
            "trace"
        );
        assert_eq!(compose_logging_filters(&explicit_cli).file_filter, "trace");
        assert_eq!(
            compose_logging_filters(&explicit_verbose).console_filter,
            "debug"
        );
        assert_eq!(
            compose_logging_filters(&explicit_verbose).file_filter,
            "debug"
        );
    }

    #[test]
    fn file_filter_adds_ffmpeg_debug_directives_when_noise_filter_omits_them() {
        let options = LoggingInitOptions {
            noise_filter: "ort=error".to_string(),
            ..Default::default()
        };

        let filters = compose_logging_filters(&options);
        assert_eq!(filters.console_filter, "ort=error,info");
        assert_eq!(
            filters.file_filter,
            "ort=error,ffmpeg_stderr=debug,ffmpeg_encode_stderr=debug,ffmpeg_stream_stderr=debug,info"
        );
    }

    #[test]
    fn redact_sensitive_text_masks_url_credentials_and_sensitive_assignments() {
        let source = "url=rtmp://alice:topsecret@example.com/live token=abc123 api_key=xyz Authorization: Bearer super-secret";
        let redacted = redact_sensitive_text(source);

        assert!(!redacted.contains("alice:topsecret"));
        assert!(!redacted.contains("abc123"));
        assert!(!redacted.contains("xyz"));
        assert!(!redacted.contains("super-secret"));
        assert!(redacted.contains(&format!(
            "url=rtmp://{REDACTION_PLACEHOLDER}@example.com/live"
        )));
        assert!(redacted.contains(&format!("token={REDACTION_PLACEHOLDER}")));
        assert!(redacted.contains(&format!("api_key={REDACTION_PLACEHOLDER}")));
        assert!(redacted.contains(&format!("Authorization: Bearer {REDACTION_PLACEHOLDER}")));
    }

    #[test]
    fn redacting_writer_redacts_across_split_writes() {
        let mut inner = Vec::new();
        {
            let mut writer = RedactingWriter::new(&mut inner);
            writer.write_all(b"token=").expect("first split write");
            writer
                .write_all(b"abc123 ffmpeg\n")
                .expect("second split write");
            writer.flush().expect("flush redacting writer");
        }

        let output = String::from_utf8(inner).expect("utf8 output");
        assert_eq!(output, format!("token={REDACTION_PLACEHOLDER} ffmpeg\n"));
    }

    #[test]
    fn file_sink_uses_default_log_dir_under_data_dir() {
        let data_dir = tempdir().expect("tempdir");
        let options = LoggingInitOptions {
            data_dir: Some(data_dir.path().to_path_buf()),
            ..Default::default()
        };

        let plan = build_file_sink_plan(&options);
        let expected_log_dir = data_dir.path().join(DEFAULT_LOG_DIR_NAME);

        match plan {
            FileSinkPlan::Ready(ready) => {
                assert_eq!(ready.log_dir, expected_log_dir);
                assert_eq!(ready.retention_files, DEFAULT_LOG_RETENTION_FILES);
                assert!(ready.log_dir.exists());
            }
            FileSinkPlan::Fallback(fallback) => panic!(
                "expected ready file sink, got fallback: {}",
                fallback.reason
            ),
        }
    }

    #[test]
    fn file_sink_wires_retention_override() {
        let data_dir = tempdir().expect("tempdir");
        let options = LoggingInitOptions {
            data_dir: Some(data_dir.path().to_path_buf()),
            retention_files: 30,
            ..Default::default()
        };

        let plan = build_file_sink_plan(&options);
        match plan {
            FileSinkPlan::Ready(ready) => assert_eq!(ready.retention_files, 30),
            FileSinkPlan::Fallback(fallback) => panic!(
                "expected ready file sink, got fallback: {}",
                fallback.reason
            ),
        }
    }

    #[test]
    fn file_sink_falls_back_when_log_dir_cannot_be_created() {
        let data_dir_file = NamedTempFile::new().expect("named temp file");
        let options = LoggingInitOptions {
            data_dir: Some(data_dir_file.path().to_path_buf()),
            ..Default::default()
        };

        let plan = build_file_sink_plan(&options);
        let expected_log_dir = data_dir_file.path().join(DEFAULT_LOG_DIR_NAME);

        match plan {
            FileSinkPlan::Ready(_) => panic!("expected fallback file sink"),
            FileSinkPlan::Fallback(fallback) => {
                assert_eq!(fallback.attempted_log_dir, Some(expected_log_dir));
                assert_eq!(fallback.retention_files, DEFAULT_LOG_RETENTION_FILES);
                assert!(fallback.reason.contains("failed to create log directory"));
            }
        }
    }

    #[test]
    fn panic_artifact_file_contains_required_sections() {
        let crash_dir = tempdir().expect("tempdir");
        let report = PanicArtifactRecord {
            timestamp: chrono::Utc::now(),
            thread_name: "test-thread".to_string(),
            source_location: "src/test.rs:12:7".to_string(),
            payload: "panic payload".to_string(),
            backtrace_policy: "captured".to_string(),
            backtrace_text: "fake backtrace".to_string(),
        };

        let artifact_path =
            write_panic_artifact_file(crash_dir.path(), &report).expect("write artifact");
        let contents = stdfs::read_to_string(&artifact_path).expect("read artifact");

        assert!(artifact_path.starts_with(crash_dir.path()));
        assert_eq!(
            artifact_path.extension().and_then(|ext| ext.to_str()),
            Some("log")
        );
        assert!(contents.contains("timestamp_utc="));
        assert!(contents.contains("thread=test-thread"));
        assert!(contents.contains("location=src/test.rs:12:7"));
        assert!(contents.contains("payload=panic payload"));
        assert!(contents.contains("backtrace_policy=captured"));
        assert!(contents.contains("backtrace:"));
        assert!(contents.contains("fake backtrace"));
    }

    #[test]
    fn panic_artifact_file_returns_error_when_directory_creation_fails() {
        let not_a_directory = NamedTempFile::new().expect("temp file");
        let crash_dir = not_a_directory.path().join("child-crash-dir");
        let report = PanicArtifactRecord {
            timestamp: chrono::Utc::now(),
            thread_name: "test-thread".to_string(),
            source_location: "src/test.rs:3:1".to_string(),
            payload: "panic payload".to_string(),
            backtrace_policy: "captured".to_string(),
            backtrace_text: "fake backtrace".to_string(),
        };

        let error = write_panic_artifact_file(&crash_dir, &report)
            .expect_err("directory creation should fail");
        assert!(!error.to_string().is_empty());
    }

    #[test]
    fn panic_payload_to_string_handles_common_payload_types() {
        let str_payload: &(dyn Any + Send) = &"boom";
        let string_payload: &(dyn Any + Send) = &"kaboom".to_string();
        let int_payload: &(dyn Any + Send) = &123_u32;

        assert_eq!(panic_payload_to_string(str_payload), "boom");
        assert_eq!(panic_payload_to_string(string_payload), "kaboom");
        assert_eq!(
            panic_payload_to_string(int_payload),
            "<non-string panic payload>"
        );
    }
}
