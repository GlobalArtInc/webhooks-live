ALTER TABLE forward_rules
  ADD COLUMN created_at timestamptz NOT NULL DEFAULT now();

ALTER TABLE forward_jobs
  ADD COLUMN append_path boolean NOT NULL DEFAULT false;

ALTER TABLE forward_attempts
  DROP COLUMN response_head,
  ADD COLUMN response_content_type text,
  ADD COLUMN response_body bytea,
  ADD COLUMN response_truncated boolean NOT NULL DEFAULT false,
  ADD CONSTRAINT forward_attempts_response_body_cap
    CHECK (response_body IS NULL OR octet_length(response_body) <= 4096);
