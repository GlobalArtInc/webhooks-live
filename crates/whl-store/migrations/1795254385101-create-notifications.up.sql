CREATE TABLE notification_channels (
  id                 uuid PRIMARY KEY,
  endpoint_id        text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  created_by         uuid REFERENCES users ON DELETE SET NULL,
  kind               text NOT NULL CHECK (kind IN ('webhook', 'slack', 'discord', 'telegram', 'email', 'push')),
  name               text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 64),
  enabled            boolean NOT NULL DEFAULT true,
  trigger            text NOT NULL CHECK (trigger IN ('capture', 'rule')),
  condition          jsonb,
  config             jsonb NOT NULL,
  secret_key_id      smallint,
  secret_nonce       bytea CHECK (secret_nonce IS NULL OR octet_length(secret_nonce) = 24),
  secret_ciphertext  bytea CHECK (secret_ciphertext IS NULL OR octet_length(secret_ciphertext) <= 4112),
  template           text CHECK (template IS NULL OR char_length(template) <= 4096),
  per_hour           integer NOT NULL CHECK (per_hour BETWEEN 1 AND 3600),
  created_at         timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT notification_channels_secret_complete CHECK (
    (secret_key_id IS NULL AND secret_nonce IS NULL AND secret_ciphertext IS NULL)
    OR (secret_key_id IS NOT NULL AND secret_nonce IS NOT NULL AND secret_ciphertext IS NOT NULL)
  )
);
CREATE INDEX notification_channels_endpoint ON notification_channels (endpoint_id);
CREATE INDEX notification_channels_capture ON notification_channels (endpoint_id)
  WHERE enabled AND trigger = 'capture';

CREATE TABLE notification_deliveries (
  id               uuid PRIMARY KEY,
  channel_id       uuid NOT NULL REFERENCES notification_channels ON DELETE CASCADE,
  capture_id       uuid REFERENCES captures (id) ON DELETE CASCADE,
  kind             text NOT NULL CHECK (kind IN ('single', 'summary', 'test')),
  message          text CHECK (message IS NULL OR char_length(message) <= 4096),
  state            text NOT NULL CHECK (state IN ('pending', 'running', 'succeeded', 'failed', 'suppressed')),
  attempts         smallint NOT NULL DEFAULT 0,
  next_attempt_at  timestamptz NOT NULL DEFAULT now(),
  locked_until     timestamptz,
  status           smallint,
  error            text CHECK (error IS NULL OR char_length(error) <= 512),
  quota_checked    boolean NOT NULL DEFAULT false,
  window_start     timestamptz,
  suppressed_count integer,
  created_at       timestamptz NOT NULL DEFAULT now(),
  finished_at      timestamptz
);
CREATE INDEX notification_deliveries_due ON notification_deliveries (next_attempt_at)
  WHERE state IN ('pending', 'running');
CREATE INDEX notification_deliveries_capture ON notification_deliveries (capture_id);
CREATE INDEX notification_deliveries_channel ON notification_deliveries (channel_id, created_at);
CREATE INDEX notification_deliveries_suppressed ON notification_deliveries (finished_at)
  WHERE state = 'suppressed';
CREATE INDEX notification_deliveries_finished_uncaptured ON notification_deliveries (finished_at)
  WHERE capture_id IS NULL AND finished_at IS NOT NULL;
CREATE UNIQUE INDEX notification_deliveries_single ON notification_deliveries (channel_id, capture_id)
  WHERE kind = 'single';
CREATE UNIQUE INDEX notification_deliveries_summary ON notification_deliveries (channel_id, window_start)
  WHERE kind = 'summary';

CREATE TABLE notification_counters (
  subject      text NOT NULL,
  window_start timestamptz NOT NULL,
  count        integer NOT NULL,
  PRIMARY KEY (subject, window_start)
);
CREATE INDEX notification_counters_window ON notification_counters (window_start);
