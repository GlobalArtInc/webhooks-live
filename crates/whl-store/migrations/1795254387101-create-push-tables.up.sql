CREATE TABLE push_subscriptions (
  id              uuid PRIMARY KEY,
  user_id         uuid NOT NULL REFERENCES users ON DELETE CASCADE,
  endpoint_url    text NOT NULL UNIQUE CHECK (char_length(endpoint_url) <= 2048),
  host            text NOT NULL CHECK (char_length(host) BETWEEN 1 AND 253),
  p256dh          bytea NOT NULL CHECK (octet_length(p256dh) = 65),
  auth            bytea NOT NULL CHECK (octet_length(auth) = 16),
  user_agent      text CHECK (user_agent IS NULL OR char_length(user_agent) <= 256),
  created_at      timestamptz NOT NULL DEFAULT now(),
  last_success_at timestamptz,
  failures        smallint NOT NULL DEFAULT 0
);
CREATE INDEX push_subscriptions_user ON push_subscriptions (user_id);

CREATE TABLE push_vapid_keys (
  id                smallint PRIMARY KEY CHECK (id = 1),
  public_key        bytea NOT NULL CHECK (octet_length(public_key) = 65),
  secret_key_id     smallint NOT NULL,
  secret_nonce      bytea NOT NULL CHECK (octet_length(secret_nonce) = 24),
  secret_ciphertext bytea NOT NULL CHECK (octet_length(secret_ciphertext) <= 4112),
  created_at        timestamptz NOT NULL DEFAULT now()
);
