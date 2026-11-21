ALTER TABLE forward_jobs
  DROP COLUMN payload;

DROP INDEX rule_runs_capture_rerun_active;
DROP INDEX rule_runs_endpoint;

ALTER TABLE rule_runs
  DROP COLUMN step_attempts,
  DROP COLUMN step_results;
