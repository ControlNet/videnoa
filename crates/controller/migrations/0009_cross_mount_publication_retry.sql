-- Cross-mount publication now has an exclusive copy fallback. Re-enable only
-- legacy capability failures with durable verified-output evidence; leave the
-- task failed until the operator explicitly retries its existing publication.
UPDATE task_attempts
SET failure_retryable = 1, version = version + 1
WHERE status = 'failed' AND failure_stage = 'publication'
  AND failure_code = 'publication_failed'
  AND failure_message = 'atomic publication cannot cross filesystems'
  AND failure_retryable = 0
  AND EXISTS (
      SELECT 1 FROM tasks
      WHERE tasks.id = task_attempts.task_id
        AND tasks.attempt_count = task_attempts.attempt_no
        AND tasks.status = 'failed' AND tasks.failure_stage = 'publication'
        AND tasks.failure_code = 'publication_failed'
        AND tasks.failure_message = 'atomic publication cannot cross filesystems'
        AND tasks.expected_output_size > 0 AND length(tasks.expected_output_sha256) = 32
  );

UPDATE tasks
SET failure_retryable = 1, version = version + 1
WHERE status = 'failed' AND failure_stage = 'publication'
  AND failure_code = 'publication_failed'
  AND failure_message = 'atomic publication cannot cross filesystems'
  AND failure_retryable = 0
  AND expected_output_size > 0 AND length(expected_output_sha256) = 32
  AND EXISTS (
      SELECT 1 FROM task_attempts
      WHERE task_attempts.task_id = tasks.id
        AND task_attempts.attempt_no = tasks.attempt_count
        AND task_attempts.status = 'failed' AND task_attempts.failure_stage = 'publication'
        AND task_attempts.failure_code = 'publication_failed'
        AND task_attempts.failure_message = 'atomic publication cannot cross filesystems'
        AND task_attempts.failure_retryable = 1
  );
