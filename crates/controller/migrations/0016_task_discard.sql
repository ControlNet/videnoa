-- An operator discarded a failed task: its files were released and it can no
-- longer be retried.
ALTER TABLE tasks ADD COLUMN discarded_at_ms INTEGER;
