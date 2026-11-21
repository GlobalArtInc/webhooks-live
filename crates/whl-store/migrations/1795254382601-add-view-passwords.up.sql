ALTER TABLE endpoints
  ADD COLUMN view_password_hash text,
  ADD CONSTRAINT endpoints_view_password_needs_no_owner CHECK (view_password_hash IS NULL OR owner_id IS NULL),
  ADD CONSTRAINT endpoints_view_password_hash_size CHECK (view_password_hash IS NULL OR char_length(view_password_hash) BETWEEN 1 AND 256);

CREATE TABLE endpoint_grants (
  token_hash   bytea PRIMARY KEY CHECK (octet_length(token_hash) = 32),
  endpoint_id  text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  created_at   timestamptz NOT NULL DEFAULT now(),
  last_used_at timestamptz NOT NULL DEFAULT now(),
  expires_at   timestamptz NOT NULL
);
CREATE INDEX endpoint_grants_endpoint ON endpoint_grants (endpoint_id);
CREATE INDEX endpoint_grants_expires ON endpoint_grants (expires_at);
