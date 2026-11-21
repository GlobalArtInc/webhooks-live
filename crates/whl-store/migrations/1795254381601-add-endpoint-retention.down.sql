DROP INDEX endpoints_never_expires;
DROP INDEX endpoints_capture_ttl;

ALTER TABLE endpoints
  DROP CONSTRAINT endpoints_never_expires_needs_owner,
  DROP COLUMN capture_ttl_seconds,
  DROP COLUMN max_captures,
  DROP COLUMN never_expires,
  DROP COLUMN retention_seconds;
