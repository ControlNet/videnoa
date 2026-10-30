-- Deleting a Worker detaches the finished attempts that ran on it. The name
-- they keep tells task history which Worker that was.
ALTER TABLE task_attempts ADD COLUMN worker_name TEXT;
