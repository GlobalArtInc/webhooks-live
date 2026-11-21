ALTER TABLE users
  DROP COLUMN endpoint_bytes_cap,
  DROP COLUMN storage_over_quota,
  DROP COLUMN bytes_used,
  DROP COLUMN tier;
