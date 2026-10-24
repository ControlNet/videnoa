-- A retried task waits in the queue for the Worker it was retried on. Deleting
-- that Worker lets any Worker take the task instead.
ALTER TABLE tasks ADD COLUMN requested_worker_id TEXT REFERENCES workers(id) ON DELETE SET NULL;
