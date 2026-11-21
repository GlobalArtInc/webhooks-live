ALTER TABLE rule_runs
  ADD COLUMN step_results  jsonb NOT NULL DEFAULT '{}',
  ADD COLUMN step_attempts smallint NOT NULL DEFAULT 0;

CREATE INDEX rule_runs_endpoint ON rule_runs (endpoint_id, created_at DESC);
CREATE UNIQUE INDEX rule_runs_capture_rerun_active ON rule_runs (capture_id)
  WHERE trigger = 'rerun' AND state IN ('queued', 'running');

ALTER TABLE forward_jobs
  ADD COLUMN payload bytea CHECK (payload IS NULL OR octet_length(payload) <= 16384);
