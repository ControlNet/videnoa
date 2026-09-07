CREATE TABLE workers (
    id TEXT PRIMARY KEY NOT NULL,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    name TEXT NOT NULL UNIQUE,
    api_url TEXT NOT NULL UNIQUE,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    online INTEGER NOT NULL DEFAULT 0 CHECK (online IN (0, 1)),
    compute_slots INTEGER NOT NULL CHECK (compute_slots BETWEEN 1 AND 65535),
    capabilities_json TEXT NOT NULL DEFAULT '{"workflows":[],"refreshed_at":null}' CHECK (json_valid(capabilities_json)),
    capabilities_refreshed_at_ms INTEGER,
    last_seen_at_ms INTEGER,
    last_assigned_at_ms INTEGER,
    health_retry_count INTEGER NOT NULL DEFAULT 0 CHECK (health_retry_count >= 0),
    next_health_check_at_ms INTEGER,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    last_error TEXT
);

CREATE TABLE tasks (
    id TEXT PRIMARY KEY NOT NULL,
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    status TEXT NOT NULL CHECK (status IN (
        'queued', 'reserved', 'uploading', 'staged', 'submitting', 'processing',
        'remote_completed', 'downloading', 'verifying', 'publishing',
        'remote_cleanup', 'completed', 'failed', 'cancelled'
    )),
    input_path TEXT NOT NULL,
    output_path TEXT NOT NULL,
    input_extension TEXT NOT NULL,
    output_extension TEXT NOT NULL,
    workflow TEXT NOT NULL,
    priority INTEGER NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('manual', 'api')),
    source_reference TEXT,
    input_size INTEGER NOT NULL CHECK (input_size >= 0),
    input_mtime_ms INTEGER NOT NULL,
    worker_id TEXT REFERENCES workers(id) ON DELETE RESTRICT,
    progress_json TEXT NOT NULL CHECK (
        json_valid(progress_json)
        AND json_type(progress_json, '$.percent') IN ('integer', 'real')
        AND json_extract(progress_json, '$.percent') BETWEEN 0 AND 100
    ),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    failure_stage TEXT CHECK (failure_stage IS NULL OR failure_stage IN (
        'reservation', 'upload', 'submission', 'processing', 'download',
        'verification', 'publication', 'local_cleanup', 'remote_cleanup'
    )),
    failure_code TEXT CHECK (failure_code IS NULL OR failure_code IN (
        'input_unavailable', 'input_changed', 'output_exists', 'worker_unavailable',
        'workflow_incompatible', 'transfer_failed', 'remote_submission_failed',
        'remote_state_ambiguous', 'processing_failed', 'verification_failed',
        'publication_failed', 'publication_ambiguous', 'cleanup_failed', 'cancelled'
    )),
    failure_message TEXT,
    failure_retryable INTEGER CHECK (failure_retryable IS NULL OR failure_retryable IN (0, 1)),
    retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
    next_retry_at_ms INTEGER,
    cancel_requested_at_ms INTEGER,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    reserved_at_ms INTEGER,
    upload_started_at_ms INTEGER,
    staged_at_ms INTEGER,
    submission_started_at_ms INTEGER,
    processing_started_at_ms INTEGER,
    remote_completed_at_ms INTEGER,
    download_started_at_ms INTEGER,
    verified_at_ms INTEGER,
    publishing_started_at_ms INTEGER,
    remote_cleanup_started_at_ms INTEGER,
    completed_at_ms INTEGER,
    expected_output_size INTEGER CHECK (expected_output_size IS NULL OR expected_output_size >= 0),
    expected_output_sha256 BLOB CHECK (expected_output_sha256 IS NULL OR length(expected_output_sha256) = 32),
    destination_staging_name TEXT,
    CHECK (
        (failure_stage IS NULL AND failure_code IS NULL AND failure_message IS NULL AND failure_retryable IS NULL)
        OR
        (failure_stage IS NOT NULL AND failure_code IS NOT NULL AND failure_message IS NOT NULL AND failure_retryable IS NOT NULL)
    )
);

CREATE TABLE task_attempts (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE RESTRICT,
    attempt_no INTEGER NOT NULL CHECK (attempt_no > 0),
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    worker_id TEXT REFERENCES workers(id) ON DELETE RESTRICT,
    status TEXT NOT NULL CHECK (status IN (
        'queued', 'reserved', 'uploading', 'staged', 'submitting', 'processing',
        'remote_completed', 'downloading', 'verifying', 'publishing',
        'remote_cleanup', 'completed', 'failed', 'cancelled'
    )),
    submission_key TEXT NOT NULL UNIQUE,
    remote_job_id TEXT,
    remote_input_path TEXT,
    remote_output_path TEXT,
    progress_json TEXT NOT NULL CHECK (
        json_valid(progress_json)
        AND json_type(progress_json, '$.percent') IN ('integer', 'real')
        AND json_extract(progress_json, '$.percent') BETWEEN 0 AND 100
    ),
    retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
    next_retry_at_ms INTEGER,
    failure_stage TEXT CHECK (failure_stage IS NULL OR failure_stage IN (
        'reservation', 'upload', 'submission', 'processing', 'download',
        'verification', 'publication', 'local_cleanup', 'remote_cleanup'
    )),
    failure_code TEXT CHECK (failure_code IS NULL OR failure_code IN (
        'input_unavailable', 'input_changed', 'output_exists', 'worker_unavailable',
        'workflow_incompatible', 'transfer_failed', 'remote_submission_failed',
        'remote_state_ambiguous', 'processing_failed', 'verification_failed',
        'publication_failed', 'publication_ambiguous', 'cleanup_failed', 'cancelled'
    )),
    failure_message TEXT,
    failure_retryable INTEGER CHECK (failure_retryable IS NULL OR failure_retryable IN (0, 1)),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    started_at_ms INTEGER,
    submitted_at_ms INTEGER,
    completed_at_ms INTEGER,
    UNIQUE (task_id, attempt_no),
    CHECK (
        (failure_stage IS NULL AND failure_code IS NULL AND failure_message IS NULL AND failure_retryable IS NULL)
        OR
        (failure_stage IS NOT NULL AND failure_code IS NOT NULL AND failure_message IS NOT NULL AND failure_retryable IS NOT NULL)
    )
);

CREATE UNIQUE INDEX idx_attempts_remote_job ON task_attempts(remote_job_id) WHERE remote_job_id IS NOT NULL;

CREATE TABLE controller_settings (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    paused INTEGER NOT NULL CHECK (paused IN (0, 1)),
    default_compute_slots INTEGER NOT NULL CHECK (default_compute_slots BETWEEN 1 AND 65535),
    prefetch_per_worker INTEGER NOT NULL CHECK (prefetch_per_worker BETWEEN 0 AND 65535),
    max_concurrent_uploads INTEGER NOT NULL CHECK (max_concurrent_uploads BETWEEN 1 AND 65535),
    max_concurrent_downloads INTEGER NOT NULL CHECK (max_concurrent_downloads BETWEEN 1 AND 65535),
    health_seconds INTEGER NOT NULL CHECK (health_seconds > 0),
    poll_seconds INTEGER NOT NULL CHECK (poll_seconds > 0),
    transfer_seconds INTEGER NOT NULL CHECK (transfer_seconds > 0),
    retry_initial_seconds INTEGER NOT NULL CHECK (retry_initial_seconds > 0),
    retry_maximum_seconds INTEGER NOT NULL CHECK (retry_maximum_seconds >= retry_initial_seconds),
    retry_max_attempts INTEGER NOT NULL CHECK (retry_max_attempts > 0),
    updated_at_ms INTEGER NOT NULL
);

INSERT INTO controller_settings (
    id, paused, default_compute_slots, prefetch_per_worker,
    max_concurrent_uploads, max_concurrent_downloads,
    health_seconds, poll_seconds, transfer_seconds,
    retry_initial_seconds, retry_maximum_seconds, retry_max_attempts, updated_at_ms
) VALUES (1, 0, 1, 1, 1, 1, 10, 5, 300, 1, 60, 5, 0);

CREATE TABLE auth_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    token_digest BLOB NOT NULL UNIQUE CHECK (length(token_digest) = 32),
    csrf_digest BLOB NOT NULL CHECK (length(csrf_digest) = 32),
    password_hash_fingerprint BLOB NOT NULL CHECK (length(password_hash_fingerprint) = 32),
    absolute_expires_at_ms INTEGER NOT NULL,
    idle_expires_at_ms INTEGER NOT NULL,
    last_used_at_ms INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL,
    revoked_at_ms INTEGER,
    CHECK (idle_expires_at_ms <= absolute_expires_at_ms)
);

CREATE TABLE task_idempotency (
    idempotency_key TEXT PRIMARY KEY NOT NULL,
    request_fingerprint BLOB NOT NULL CHECK (length(request_fingerprint) = 32),
    task_id TEXT NOT NULL UNIQUE REFERENCES tasks(id) ON DELETE RESTRICT,
    created_at_ms INTEGER NOT NULL
);

CREATE INDEX idx_tasks_status_created ON tasks(status, created_at_ms, id);
CREATE INDEX idx_tasks_completed ON tasks(completed_at_ms, id);
CREATE INDEX idx_tasks_worker ON tasks(worker_id, created_at_ms, id);
CREATE INDEX idx_tasks_worker_status ON tasks(worker_id, status, id);
CREATE INDEX idx_tasks_workflow ON tasks(workflow, created_at_ms, id);
CREATE INDEX idx_tasks_source ON tasks(source, created_at_ms, id);
CREATE INDEX idx_tasks_failure_stage ON tasks(failure_stage, created_at_ms, id) WHERE failure_stage IS NOT NULL;
CREATE INDEX idx_tasks_queue ON tasks(status, priority DESC, created_at_ms ASC, id ASC) WHERE status = 'queued';
CREATE INDEX idx_tasks_retry_wakeup ON tasks(next_retry_at_ms, id) WHERE next_retry_at_ms IS NOT NULL;
CREATE INDEX idx_tasks_priority_sort ON tasks(priority DESC, created_at_ms ASC, id ASC);
CREATE INDEX idx_tasks_created_sort ON tasks(created_at_ms, id);
CREATE INDEX idx_tasks_status_sort ON tasks(status, id);
CREATE INDEX idx_tasks_worker_sort ON tasks(worker_id, id);
CREATE INDEX idx_tasks_duration_sort ON tasks((COALESCE(completed_at_ms, updated_at_ms) - created_at_ms), id);
CREATE INDEX idx_tasks_recovery ON tasks(updated_at_ms, id) WHERE status NOT IN ('completed', 'failed', 'cancelled');
CREATE INDEX idx_attempts_task_history ON task_attempts(task_id, attempt_no DESC);
CREATE INDEX idx_attempts_retry_wakeup ON task_attempts(next_retry_at_ms, id) WHERE next_retry_at_ms IS NOT NULL;
CREATE INDEX idx_attempts_recovery ON task_attempts(status, updated_at_ms, id) WHERE status NOT IN ('completed', 'failed', 'cancelled');
CREATE INDEX idx_workers_assignment ON workers(enabled, online, last_assigned_at_ms, id);
CREATE INDEX idx_workers_health_wakeup ON workers(next_health_check_at_ms, id) WHERE next_health_check_at_ms IS NOT NULL;
CREATE INDEX idx_sessions_absolute_expiry ON auth_sessions(absolute_expires_at_ms, id);
CREATE INDEX idx_sessions_idle_expiry ON auth_sessions(idle_expires_at_ms, id);
