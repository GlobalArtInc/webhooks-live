ALTER TABLE endpoints
  ADD COLUMN rules_version integer NOT NULL DEFAULT 0,
  ADD COLUMN rule_count    smallint NOT NULL DEFAULT 0,
  ADD COLUMN rules_sync    boolean NOT NULL DEFAULT false;

CREATE TABLE rules (
  id          uuid PRIMARY KEY,
  endpoint_id text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  position    smallint NOT NULL,
  name        text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
  enabled     boolean NOT NULL DEFAULT true,
  condition   jsonb NOT NULL DEFAULT '{}',
  steps       jsonb NOT NULL,
  created_at  timestamptz NOT NULL DEFAULT now(),
  updated_at  timestamptz NOT NULL DEFAULT now(),
  UNIQUE (endpoint_id, position) DEFERRABLE INITIALLY DEFERRED
);
