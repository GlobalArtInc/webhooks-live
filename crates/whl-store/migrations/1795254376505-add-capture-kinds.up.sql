ALTER TABLE captures
  ADD COLUMN kind text NOT NULL DEFAULT 'http' CHECK (kind IN ('http', 'email', 'dns', 'schedule')),
  ADD COLUMN meta jsonb NOT NULL DEFAULT '{}';
