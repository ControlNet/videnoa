CREATE TABLE administrator_credential (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    password_hash TEXT NOT NULL CHECK (
        length(password_hash) BETWEEN 1 AND 4096
        AND password_hash LIKE '$argon2id$%'
    ),
    created_at_ms INTEGER NOT NULL
);
