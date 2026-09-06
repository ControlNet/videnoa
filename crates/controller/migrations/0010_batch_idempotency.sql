CREATE TABLE batch_idempotency (
    idempotency_key TEXT PRIMARY KEY NOT NULL,
    request_fingerprint BLOB NOT NULL CHECK(length(request_fingerprint) = 32),
    response_status INTEGER,
    response_json TEXT,
    created_at_ms INTEGER NOT NULL,
    CHECK ((response_status IS NULL AND response_json IS NULL)
        OR (response_status IN (201, 207) AND json_valid(response_json)))
);
