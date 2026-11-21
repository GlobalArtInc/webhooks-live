ALTER TABLE users
  ADD COLUMN tier               text NOT NULL DEFAULT 'member' CHECK (tier ~ '^[a-z][a-z0-9_-]{0,31}$'),
  ADD COLUMN bytes_used         bigint NOT NULL DEFAULT 0 CHECK (bytes_used >= 0),
  ADD COLUMN storage_over_quota boolean NOT NULL DEFAULT false,
  ADD COLUMN endpoint_bytes_cap bigint CHECK (endpoint_bytes_cap >= 0);
