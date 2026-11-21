ALTER TABLE endpoints
  ADD COLUMN retention_seconds   integer CHECK (retention_seconds >= 3600),
  ADD COLUMN never_expires       boolean NOT NULL DEFAULT false,
  ADD COLUMN max_captures        integer CHECK (max_captures > 0),
  ADD COLUMN capture_ttl_seconds integer CHECK (capture_ttl_seconds >= 60),
  ADD CONSTRAINT endpoints_never_expires_needs_owner CHECK (NOT never_expires OR owner_id IS NOT NULL);

CREATE INDEX endpoints_capture_ttl ON endpoints (id) WHERE capture_ttl_seconds IS NOT NULL AND deleted_at IS NULL;
CREATE INDEX endpoints_never_expires ON endpoints (owner_id) WHERE never_expires AND deleted_at IS NULL;
