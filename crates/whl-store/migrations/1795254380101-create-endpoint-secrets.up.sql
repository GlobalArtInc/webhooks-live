CREATE TABLE endpoint_secrets (
  endpoint_id text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  name        text NOT NULL CHECK (name ~ '^[A-Z][A-Z0-9_]{0,63}$'),
  key_id      smallint NOT NULL,
  nonce       bytea NOT NULL CHECK (octet_length(nonce) = 24),
  ciphertext  bytea NOT NULL CHECK (octet_length(ciphertext) <= 4112),
  created_at  timestamptz NOT NULL DEFAULT now(),
  updated_at  timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (endpoint_id, name)
);
