CREATE TABLE users (
  id            uuid PRIMARY KEY,
  issuer        text NOT NULL,
  subject       text NOT NULL,
  email         text NOT NULL,
  display_name  text,
  created_at    timestamptz NOT NULL DEFAULT now(),
  last_login_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (issuer, subject)
);
CREATE INDEX users_email ON users (lower(email));

CREATE TABLE sessions (
  token_hash   bytea PRIMARY KEY,
  user_id      uuid NOT NULL REFERENCES users ON DELETE CASCADE,
  created_at   timestamptz NOT NULL DEFAULT now(),
  last_seen_at timestamptz NOT NULL DEFAULT now(),
  expires_at   timestamptz NOT NULL
);
CREATE INDEX sessions_expires ON sessions (expires_at);

CREATE TABLE login_states (
  state         text PRIMARY KEY,
  pkce_verifier text NOT NULL,
  nonce         text NOT NULL,
  return_to     text NOT NULL,
  expires_at    timestamptz NOT NULL
);
