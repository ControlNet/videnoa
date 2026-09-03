DROP INDEX idx_attempts_remote_job;

CREATE UNIQUE INDEX idx_attempts_worker_remote_job
ON task_attempts(worker_id, remote_job_id)
WHERE worker_id IS NOT NULL AND remote_job_id IS NOT NULL;
