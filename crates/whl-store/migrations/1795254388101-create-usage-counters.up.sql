CREATE TABLE usage_counters (
  subject      text NOT NULL,
  metric       text NOT NULL,
  window_start timestamptz NOT NULL,
  count        bigint NOT NULL,
  PRIMARY KEY (subject, metric, window_start)
);
CREATE INDEX usage_counters_window ON usage_counters (window_start);
