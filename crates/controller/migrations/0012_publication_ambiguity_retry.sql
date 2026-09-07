-- Permit explicit publication recovery for existing ambiguous failures. Keep
-- tasks failed and preserve all evidence; retry must re-run publication checks.
UPDATE task_attempts
SET failure_retryable = 1, version = version + 1
WHERE status = 'failed' AND failure_stage = 'publication'
  AND failure_code = 'publication_ambiguous' AND failure_retryable = 0
  AND EXISTS (
      SELECT 1 FROM tasks
      WHERE tasks.id = task_attempts.task_id
        AND tasks.attempt_count = task_attempts.attempt_no
        AND tasks.status = 'failed' AND tasks.failure_stage = 'publication'
        AND tasks.failure_code = 'publication_ambiguous'
  );

UPDATE tasks
SET failure_retryable = 1, version = version + 1
WHERE status = 'failed' AND failure_stage = 'publication'
  AND failure_code = 'publication_ambiguous' AND failure_retryable = 0
  AND EXISTS (
      SELECT 1 FROM task_attempts
      WHERE task_attempts.task_id = tasks.id
        AND task_attempts.attempt_no = tasks.attempt_count
        AND task_attempts.status = 'failed' AND task_attempts.failure_stage = 'publication'
        AND task_attempts.failure_code = 'publication_ambiguous'
        AND task_attempts.failure_retryable = 1
  );
