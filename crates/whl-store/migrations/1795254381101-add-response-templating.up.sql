ALTER TABLE endpoints
  ADD COLUMN response_templated boolean NOT NULL DEFAULT false,
  ADD COLUMN response_status_template text CHECK (char_length(response_status_template) <= 1024);

CREATE TABLE endpoint_vars (
  endpoint_id text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  name        text NOT NULL CHECK (name ~ '^[a-z_][a-z0-9_]{0,63}$'),
  value       jsonb NOT NULL CHECK (octet_length(value::text) <= 8192),
  updated_at  timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (endpoint_id, name)
);
