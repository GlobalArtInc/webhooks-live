CREATE TABLE api_tokens (
  id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id      uuid NOT NULL REFERENCES users ON DELETE CASCADE,
  name         text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 64),
  prefix       text NOT NULL CHECK (char_length(prefix) = 12),
  token_hash   bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
  scopes       text[] NOT NULL CHECK (
    cardinality(scopes) BETWEEN 1 AND 5
    AND scopes <@ ARRAY['read', 'endpoints:write', 'captures:write', 'rules:write', 'send']::text[]
  ),
  endpoint_ids text[] CHECK (endpoint_ids IS NULL OR cardinality(endpoint_ids) BETWEEN 1 AND 50),
  created_at   timestamptz NOT NULL DEFAULT now(),
  expires_at   timestamptz NOT NULL,
  last_used_at timestamptz,
  revoked_at   timestamptz
);
CREATE INDEX api_tokens_user ON api_tokens (user_id);
CREATE INDEX api_tokens_prefix ON api_tokens (prefix);
