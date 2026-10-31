PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE _sqlx_migrations (
    version BIGINT PRIMARY KEY,
    description TEXT NOT NULL,
    installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    success BOOLEAN NOT NULL,
    checksum BLOB NOT NULL,
    execution_time BIGINT NOT NULL
);
INSERT INTO _sqlx_migrations VALUES(1,'controller state','2026-10-30 09:52:59',1,X'1d42501ca561035705475bbaef223a1235aad8f5338818e9e4e88d0759f9e326f76c194b91a0f51f989f23a4e540bc72',12944890);
INSERT INTO _sqlx_migrations VALUES(2,'task input identity','2026-10-30 09:52:59',1,X'4c8aa9d2f30b926d61f99c780e3a1535742dcea1819c53023c267f4b4ac74089ccc9effe21116fad09c0ffd421c2a3a5',8131038);
INSERT INTO _sqlx_migrations VALUES(3,'worker registry','2026-10-30 09:52:59',1,X'1cea37d06f13cd89d56579c490d54f12db8df2cd484e05066d58ebc471c238f4cd78a8ab551e7d971cbc55a0b959c9dd',6555490);
INSERT INTO _sqlx_migrations VALUES(4,'task input content identity','2026-10-30 09:52:59',1,X'9cc525e9b7f67055e61245f96d59bf866da56cceb77edbd32f0a2c396fdc9f6bd430c9f63aa7205d0db318651fefe6ae',8028510);
INSERT INTO _sqlx_migrations VALUES(5,'worker scoped remote job','2026-10-30 09:52:59',1,X'42a5afbf4ac15a31e9fa73dc7cceecdb57b507c09fa7cd63afd32bfa69e1e002141d920d13f0c6cfa27a5d0aa2ebd009',7525013);
INSERT INTO _sqlx_migrations VALUES(6,'submission ownership','2026-10-30 09:52:59',1,X'e7cf37e286b8be32bd209317a297e1f4e49e6e36cad043068709b7ae09f1b9a2d2fe6efa5dd29f6315a6922499810374',6964624);
INSERT INTO _sqlx_migrations VALUES(7,'configuration projection','2026-10-30 09:52:59',1,X'c32b52220e857b4b2c1c9821a929ad846c51da41e78c8e2f744e81e8b07ca0630f87af44777ea574d040a2fdca0809c2',13859829);
INSERT INTO _sqlx_migrations VALUES(8,'administrator credential','2026-10-30 09:52:59',1,X'dcc92ff6a98f5ad624f50c2f89b2c5ee89ec945b0e5a38c6f56b306838c867723d92d2deea64d4918104bc78fc6423ea',35286133);
INSERT INTO _sqlx_migrations VALUES(9,'cross mount publication retry','2026-10-30 09:52:59',1,X'3d56d5e26facce9b815755e1465dabf5537f6972a5864c54a173ef5f23159a0053db5f63c5e31c70df355e5e549f034f',9129036);
INSERT INTO _sqlx_migrations VALUES(10,'batch idempotency','2026-10-30 09:52:59',1,X'187a64bbd512078b7cbe451b6e27e0f8e4082171207ecc2a248a2d96714d6c573a667d812783913afce172eb1cb44883',5822803);
INSERT INTO _sqlx_migrations VALUES(11,'worker password','2026-10-30 09:52:59',1,X'ddb9d39450b33077b5587ec4237bb8a6d51f34a4b5110b89428bdf7c097589c671b2d0ec2c53ddd6f04d555c4aec5cf8',7411210);
INSERT INTO _sqlx_migrations VALUES(12,'publication ambiguity retry','2026-10-30 09:52:59',1,X'635e9cda885a62a3b2ff6367a1bdd16af1444a4907e32e160902278fdfe83633c69767fa90e5e3dcb2ba66548a125a99',5794992);
INSERT INTO _sqlx_migrations VALUES(13,'iroh endpoints','2026-10-30 09:52:59',1,X'91ccf1faa3cd9015f9a9f7aae37856ecebb2f96dca01b887574545c76cfb341a98eadbd1788d656af1989916f80c69e0',25297677);
INSERT INTO _sqlx_migrations VALUES(14,'task requested worker','2026-10-30 09:52:59',1,X'63e4bc83f84dc637a442f8df9c611d59e535a294e1a6c2b053685a4bb60813d17aa725d4fc97216d2a2cd780f09a8caa',7874734);
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
, password TEXT, transport TEXT GENERATED ALWAYS AS
    (CASE WHEN substr(api_url, 1, 7) = 'iroh://' THEN 'iroh' ELSE 'http' END) VIRTUAL, endpoint_id TEXT GENERATED ALWAYS AS
    (CASE WHEN substr(api_url, 1, 7) = 'iroh://' THEN substr(api_url, 8, length(api_url) - 8) ELSE NULL END) VIRTUAL);
INSERT INTO workers VALUES('11111111-1111-4111-8111-111111111111',0,'fixture-worker','http://127.0.0.1:9/',1,0,2,'{"workflows":[],"refreshed_at":null}',NULL,NULL,NULL,0,NULL,1700000000000,1700000000000,'worker health check failed during recovery',NULL);
INSERT INTO workers VALUES('22222222-2222-4222-8222-222222222222',0,'retired-worker','http://127.0.0.1:10/',0,0,1,'{"workflows":[],"refreshed_at":null}',NULL,NULL,NULL,0,NULL,1700000000000,1700000000000,NULL,NULL);
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
    destination_staging_name TEXT, input_identity BLOB
    CHECK (input_identity IS NULL OR length(input_identity) = 16), input_content_identity BLOB
    CHECK (input_content_identity IS NULL OR length(input_content_identity) = 16), requested_worker_id TEXT REFERENCES workers(id) ON DELETE SET NULL,
    CHECK (
        (failure_stage IS NULL AND failure_code IS NULL AND failure_message IS NULL AND failure_retryable IS NULL)
        OR
        (failure_stage IS NOT NULL AND failure_code IS NOT NULL AND failure_message IS NOT NULL AND failure_retryable IS NOT NULL)
    )
);
INSERT INTO tasks VALUES('aaaaaaaa-0000-4000-8000-000000000001',7,'completed','/media/in/done.mkv','/media/out/done.mp4','mkv','mp4','interpolation-2x',0,'manual',NULL,1024,1700000000000,'11111111-1111-4111-8111-111111111111','{"percent":100}',1,NULL,NULL,NULL,NULL,0,NULL,NULL,1700000000000,1700000100000,1700000001000,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,1700000100000,NULL,NULL,NULL,X'00112233445566778899aabbccddeeff',X'ffeeddccbbaa99887766554433221100',NULL);
INSERT INTO tasks VALUES('aaaaaaaa-0000-4000-8000-000000000003',4,'processing','/media/in/busy.mkv','/media/out/busy.mp4','mkv','mp4','interpolation-2x',5,'api','jellyfin:42',2048,1700000000000,'11111111-1111-4111-8111-111111111111','{"percent":40,"processed_frames":400,"total_frames":1000}',1,NULL,NULL,NULL,NULL,0,NULL,NULL,1700000000000,1700000200000,1700000001000,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL);
INSERT INTO tasks VALUES('aaaaaaaa-0000-4000-8000-000000000004',0,'queued','/media/in/wait.mkv','/media/out/wait.mp4','mkv','mp4','interpolation-2x',0,'manual',NULL,10,1700000000000,NULL,'{"percent":0}',0,NULL,NULL,NULL,NULL,0,NULL,NULL,1700000000000,1700000000000,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL);
INSERT INTO tasks VALUES('aaaaaaaa-0000-4000-8000-000000000005',2,'cancelled','/media/in/stop.mkv','/media/out/stop.mp4','mkv','mp4','interpolation-2x',0,'manual',NULL,10,1700000000000,NULL,'{"percent":0}',0,NULL,NULL,NULL,NULL,0,NULL,NULL,1700000000000,1700000300000,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL);
INSERT INTO tasks VALUES('aaaaaaaa-0000-4000-8000-000000000002',9,'failed','/media/in/crash.mkv','/media/out/crash.mp4','mkv','mp4','interpolation-2x',0,'manual',NULL,4096,1700000000000,'11111111-1111-4111-8111-111111111111','{"percent":10}',1,'processing','processing_failed','remote job failed',1,0,NULL,NULL,1700000000000,1700000400000,1700000001000,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL);
INSERT INTO tasks VALUES('aaaaaaaa-0000-4000-8000-000000000006',11,'failed','/media/in/clash.mkv','/media/out/clash.mp4','mkv','mp4','interpolation-2x',0,'manual',NULL,4096,1700000000000,'11111111-1111-4111-8111-111111111111','{"percent":100}',1,'publication','output_exists','destination already exists',0,0,NULL,NULL,1700000000000,1700000500000,1700000001000,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,8,X'0000000000000000000000000000000000000000000000000000000000000001',NULL,NULL,NULL,NULL);
INSERT INTO tasks VALUES('aaaaaaaa-0000-4000-8000-000000000007',6,'queued','/media/in/pinned.mkv','/media/out/pinned.mp4','mkv','mp4','interpolation-2x',0,'manual',NULL,4096,1700000000000,NULL,'{"percent":0}',1,NULL,NULL,NULL,NULL,0,NULL,NULL,1700000000000,1700000600000,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,'22222222-2222-4222-8222-222222222222');
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
    completed_at_ms INTEGER, submission_owner TEXT,
    UNIQUE (task_id, attempt_no),
    CHECK (
        (failure_stage IS NULL AND failure_code IS NULL AND failure_message IS NULL AND failure_retryable IS NULL)
        OR
        (failure_stage IS NOT NULL AND failure_code IS NOT NULL AND failure_message IS NOT NULL AND failure_retryable IS NOT NULL)
    )
);
INSERT INTO task_attempts VALUES('bbbbbbbb-0000-4000-8000-000000000001','aaaaaaaa-0000-4000-8000-000000000001',1,9,'11111111-1111-4111-8111-111111111111','completed','cccccccc-0000-4000-8000-000000000001','dddddddd-0000-4000-8000-000000000001','aaaaaaaa-0000-4000-8000-000000000001/input.mkv','aaaaaaaa-0000-4000-8000-000000000001/output.mp4','{"percent":100}',0,NULL,NULL,NULL,NULL,NULL,1700000001000,1700000100000,1700000002000,NULL,1700000100000,NULL);
INSERT INTO task_attempts VALUES('bbbbbbbb-0000-4000-8000-000000000003','aaaaaaaa-0000-4000-8000-000000000003',1,4,'11111111-1111-4111-8111-111111111111','processing','cccccccc-0000-4000-8000-000000000003','dddddddd-0000-4000-8000-000000000003','aaaaaaaa-0000-4000-8000-000000000003/input.mkv','aaaaaaaa-0000-4000-8000-000000000003/output.mp4','{"percent":40}',0,NULL,NULL,NULL,NULL,NULL,1700000001000,1700000200000,1700000002000,NULL,NULL,NULL);
INSERT INTO task_attempts VALUES('bbbbbbbb-0000-4000-8000-000000000002','aaaaaaaa-0000-4000-8000-000000000002',1,5,'11111111-1111-4111-8111-111111111111','failed','cccccccc-0000-4000-8000-000000000002','dddddddd-0000-4000-8000-000000000002','aaaaaaaa-0000-4000-8000-000000000002/input.mkv','aaaaaaaa-0000-4000-8000-000000000002/output.mp4','{"percent":10}',0,NULL,'processing','processing_failed','remote job failed',1,1700000001000,1700000400000,1700000002000,NULL,1700000400000,NULL);
INSERT INTO task_attempts VALUES('bbbbbbbb-0000-4000-8000-000000000006','aaaaaaaa-0000-4000-8000-000000000006',1,8,'11111111-1111-4111-8111-111111111111','failed','cccccccc-0000-4000-8000-000000000006','dddddddd-0000-4000-8000-000000000006','aaaaaaaa-0000-4000-8000-000000000006/input.mkv','aaaaaaaa-0000-4000-8000-000000000006/output.mp4','{"percent":100}',0,NULL,'publication','output_exists','destination already exists',0,1700000001000,1700000500000,1700000002000,NULL,1700000500000,NULL);
INSERT INTO task_attempts VALUES('bbbbbbbb-0000-4000-8000-000000000007','aaaaaaaa-0000-4000-8000-000000000007',1,5,'22222222-2222-4222-8222-222222222222','failed','cccccccc-0000-4000-8000-000000000007','dddddddd-0000-4000-8000-000000000007',NULL,NULL,'{"percent":0}',0,NULL,'processing','processing_failed','remote job failed',1,1700000001000,1700000600000,1700000002000,NULL,1700000600000,NULL);
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
, server_host TEXT NOT NULL DEFAULT '127.0.0.1', server_port INTEGER NOT NULL DEFAULT 3001 CHECK (server_port BETWEEN 1 AND 65535), secure_cookie INTEGER NOT NULL DEFAULT 0 CHECK (secure_cookie IN (0, 1)), session_absolute_seconds INTEGER NOT NULL DEFAULT 86400 CHECK (session_absolute_seconds > 0), session_idle_seconds INTEGER NOT NULL DEFAULT 3600 CHECK (session_idle_seconds > 0 AND session_idle_seconds <= session_absolute_seconds), config_document TEXT NOT NULL DEFAULT '', pending_config_document TEXT, configuration_initialized INTEGER NOT NULL DEFAULT 0 CHECK (configuration_initialized IN (0, 1)));
INSERT INTO controller_settings VALUES(1,0,0,1,1,1,1,10,5,300,1,60,5,0,'127.0.0.1',3001,0,86400,3600,'',NULL,0);
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
INSERT INTO task_idempotency VALUES('fixture-task-key',X'0101010101010101010101010101010101010101010101010101010101010101','aaaaaaaa-0000-4000-8000-000000000001',1700000000000);
CREATE TABLE administrator_credential (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    password_hash TEXT NOT NULL CHECK (
        length(password_hash) BETWEEN 1 AND 4096
        AND password_hash LIKE '$argon2id$%'
    ),
    created_at_ms INTEGER NOT NULL
);
CREATE TABLE batch_idempotency (
    idempotency_key TEXT PRIMARY KEY NOT NULL,
    request_fingerprint BLOB NOT NULL CHECK(length(request_fingerprint) = 32),
    response_status INTEGER,
    response_json TEXT,
    created_at_ms INTEGER NOT NULL,
    CHECK ((response_status IS NULL AND response_json IS NULL)
        OR (response_status IN (201, 207) AND json_valid(response_json)))
);
INSERT INTO batch_idempotency VALUES('fixture-batch-key',X'0202020202020202020202020202020202020202020202020202020202020202',201,'{"items":[]}',1700000000000);
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
CREATE INDEX idx_tasks_recovery ON tasks(updated_at_ms, id) WHERE status NOT IN ('completed', 'failed', 'cancelled');
CREATE INDEX idx_attempts_task_history ON task_attempts(task_id, attempt_no DESC);
CREATE INDEX idx_attempts_retry_wakeup ON task_attempts(next_retry_at_ms, id) WHERE next_retry_at_ms IS NOT NULL;
CREATE INDEX idx_attempts_recovery ON task_attempts(status, updated_at_ms, id) WHERE status NOT IN ('completed', 'failed', 'cancelled');
CREATE INDEX idx_workers_assignment ON workers(enabled, online, last_assigned_at_ms, id);
CREATE INDEX idx_workers_health_wakeup ON workers(next_health_check_at_ms, id) WHERE next_health_check_at_ms IS NOT NULL;
CREATE INDEX idx_sessions_absolute_expiry ON auth_sessions(absolute_expires_at_ms, id);
CREATE INDEX idx_sessions_idle_expiry ON auth_sessions(idle_expires_at_ms, id);
CREATE INDEX idx_tasks_duration_sort
    ON tasks((completed_at_ms - created_at_ms), id);
CREATE UNIQUE INDEX idx_workers_name_normalized
ON workers(lower(trim(name)));
CREATE UNIQUE INDEX idx_attempts_worker_remote_job
ON task_attempts(worker_id, remote_job_id)
WHERE worker_id IS NOT NULL AND remote_job_id IS NOT NULL;
CREATE UNIQUE INDEX workers_iroh_endpoint_id ON workers(endpoint_id) WHERE endpoint_id IS NOT NULL;
COMMIT;
