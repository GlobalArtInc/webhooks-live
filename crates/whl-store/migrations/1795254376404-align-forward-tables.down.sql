ALTER TABLE forward_attempts
  DROP CONSTRAINT forward_attempts_response_body_cap,
  DROP COLUMN response_truncated,
  DROP COLUMN response_body,
  DROP COLUMN response_content_type,
  ADD COLUMN response_head bytea;

ALTER TABLE forward_jobs
  DROP COLUMN append_path;

ALTER TABLE forward_rules
  DROP COLUMN created_at;
