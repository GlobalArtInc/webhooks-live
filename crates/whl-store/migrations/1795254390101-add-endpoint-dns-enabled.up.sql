ALTER TABLE endpoints
  ADD COLUMN dns_enabled boolean NOT NULL DEFAULT false;
