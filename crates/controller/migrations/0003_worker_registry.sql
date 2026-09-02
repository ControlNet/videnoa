CREATE UNIQUE INDEX idx_workers_name_normalized
ON workers(lower(trim(name)));
