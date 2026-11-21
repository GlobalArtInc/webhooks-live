ALTER TABLE endpoints
  ADD COLUMN email_enabled boolean NOT NULL DEFAULT false,
  ADD COLUMN sender_allow_cidrs cidr[] NOT NULL DEFAULT '{}' CHECK (cardinality(sender_allow_cidrs) <= 20);
