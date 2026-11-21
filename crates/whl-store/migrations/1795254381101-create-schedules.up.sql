CREATE TABLE schedules (
  id                   uuid PRIMARY KEY,
  endpoint_id          text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  created_by           uuid REFERENCES users ON DELETE SET NULL,
  name                 text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
  cron                 text NOT NULL CHECK (char_length(cron) <= 256),
  timezone             text NOT NULL DEFAULT 'UTC' CHECK (char_length(timezone) <= 64),
  request              jsonb NOT NULL,
  enabled              boolean NOT NULL DEFAULT true,
  paused_reason        text CHECK (char_length(paused_reason) <= 500),
  next_run_at          timestamptz,
  last_run_at          timestamptz,
  consecutive_failures smallint NOT NULL DEFAULT 0,
  locked_until         timestamptz,
  created_at           timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX schedules_due ON schedules (next_run_at) WHERE enabled;
CREATE INDEX schedules_endpoint ON schedules (endpoint_id);

CREATE TABLE schedule_runs (
  id          uuid PRIMARY KEY,
  schedule_id uuid NOT NULL REFERENCES schedules ON DELETE CASCADE,
  started_at  timestamptz NOT NULL,
  duration_ms integer NOT NULL CHECK (duration_ms >= 0),
  outcome     text NOT NULL CHECK (outcome IN ('succeeded', 'failed', 'skipped')),
  status      smallint,
  error       text CHECK (char_length(error) <= 500),
  capture_id  uuid
);
CREATE INDEX schedule_runs_schedule ON schedule_runs (schedule_id, started_at DESC, id DESC);

CREATE TABLE schedule_quota_windows (
  owner_id     uuid NOT NULL REFERENCES users ON DELETE CASCADE,
  window_start timestamptz NOT NULL,
  count        integer NOT NULL CHECK (count >= 0),
  PRIMARY KEY (owner_id, window_start)
);
