ALTER TABLE endpoints
  ADD COLUMN sender_allow_cidrs cidr[] NOT NULL DEFAULT '{}' CHECK (cardinality(sender_allow_cidrs) <= 20),
  ADD COLUMN capture_auth       jsonb;
