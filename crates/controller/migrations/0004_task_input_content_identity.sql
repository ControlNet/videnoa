ALTER TABLE tasks ADD COLUMN input_content_identity BLOB
    CHECK (input_content_identity IS NULL OR length(input_content_identity) = 16);
