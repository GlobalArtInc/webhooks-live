CREATE TABLE sent_requests (
  id                uuid PRIMARY KEY,
  user_id           uuid REFERENCES users ON DELETE CASCADE,
  endpoint_id       text REFERENCES endpoints ON DELETE CASCADE,
  source_capture_id uuid,
  created_at        timestamptz NOT NULL DEFAULT now(),
  request           jsonb NOT NULL,
  result            jsonb NOT NULL,
  response_body     bytea CHECK (octet_length(response_body) <= 65536),
  CHECK (user_id IS NOT NULL OR endpoint_id IS NOT NULL)
);
CREATE INDEX sent_requests_user ON sent_requests (user_id, created_at DESC) WHERE user_id IS NOT NULL;
CREATE INDEX sent_requests_endpoint ON sent_requests (endpoint_id, created_at DESC) WHERE endpoint_id IS NOT NULL;
