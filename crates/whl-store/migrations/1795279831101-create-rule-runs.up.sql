ALTER TABLE captures
  ADD COLUMN rule_state text CHECK (rule_state IN ('running', 'succeeded', 'failed'));

CREATE TABLE rule_runs (
  id              uuid PRIMARY KEY,
  capture_id      uuid NOT NULL REFERENCES captures (id) ON DELETE CASCADE,
  endpoint_id     text NOT NULL,
  trigger         text NOT NULL CHECK (trigger IN ('capture', 'rerun')),
  rules_version   integer NOT NULL,
  state           text NOT NULL CHECK (state IN ('sync', 'queued', 'running', 'succeeded', 'failed')),
  attempts        smallint NOT NULL DEFAULT 0,
  next_attempt_at timestamptz NOT NULL DEFAULT now(),
  locked_until    timestamptz,
  plan            jsonb,
  next_step       smallint NOT NULL DEFAULT 0,
  vars            jsonb NOT NULL DEFAULT '{}',
  log             jsonb NOT NULL DEFAULT '[]',
  response_status smallint,
  error           text,
  created_at      timestamptz NOT NULL DEFAULT now(),
  finished_at     timestamptz
);
CREATE INDEX rule_runs_due ON rule_runs (next_attempt_at) WHERE state IN ('sync', 'queued', 'running');
CREATE INDEX rule_runs_capture ON rule_runs (capture_id);
CREATE UNIQUE INDEX rule_runs_capture_trigger ON rule_runs (capture_id) WHERE trigger = 'capture';
