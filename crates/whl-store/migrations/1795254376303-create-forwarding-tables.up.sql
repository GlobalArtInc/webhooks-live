CREATE TABLE forward_rules (
  id               uuid PRIMARY KEY,
  endpoint_id      text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  position         smallint NOT NULL,
  enabled          boolean NOT NULL DEFAULT true,
  target_url       text NOT NULL,
  method           text,
  content_type     text,
  header_allowlist text[] NOT NULL DEFAULT '{}',
  append_path      boolean NOT NULL DEFAULT false,
  UNIQUE (endpoint_id, position)
);

CREATE TABLE forward_jobs (
  id               uuid PRIMARY KEY,
  capture_id       uuid NOT NULL REFERENCES captures (id) ON DELETE CASCADE,
  rule_id          uuid REFERENCES forward_rules ON DELETE SET NULL,
  kind             text NOT NULL CHECK (kind IN ('rule', 'manual')),
  target_url       text NOT NULL,
  method           text NOT NULL,
  content_type     text,
  header_allowlist text[] NOT NULL,
  state            text NOT NULL CHECK (state IN ('pending', 'running', 'succeeded', 'failed')),
  attempts         smallint NOT NULL DEFAULT 0,
  next_attempt_at  timestamptz NOT NULL DEFAULT now(),
  locked_until     timestamptz,
  created_at       timestamptz NOT NULL DEFAULT now(),
  finished_at      timestamptz
);
CREATE INDEX forward_jobs_due ON forward_jobs (next_attempt_at) WHERE state IN ('pending', 'running');
CREATE INDEX forward_jobs_capture ON forward_jobs (capture_id);

CREATE TABLE forward_attempts (
  job_id       uuid NOT NULL REFERENCES forward_jobs ON DELETE CASCADE,
  attempt      smallint NOT NULL,
  started_at   timestamptz NOT NULL,
  duration_ms  integer NOT NULL,
  status       smallint,
  error        text,
  final_url    text,
  response_head bytea,
  PRIMARY KEY (job_id, attempt)
);
