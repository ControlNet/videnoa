ALTER TABLE tasks ADD COLUMN input_identity BLOB
    CHECK (input_identity IS NULL OR length(input_identity) = 16);

DROP INDEX idx_tasks_duration_sort;

CREATE INDEX idx_tasks_duration_sort
    ON tasks((completed_at_ms - created_at_ms), id);
