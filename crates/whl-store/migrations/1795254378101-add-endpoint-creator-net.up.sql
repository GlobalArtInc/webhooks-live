ALTER TABLE endpoints
  ADD COLUMN creator_net cidr;

CREATE INDEX endpoints_creator_net ON endpoints (creator_net) WHERE owner_id IS NULL AND deleted_at IS NULL;
