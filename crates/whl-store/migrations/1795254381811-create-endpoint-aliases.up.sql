CREATE TABLE endpoint_aliases (
  alias       text PRIMARY KEY CHECK (alias ~ '^[a-z][a-z0-9-]{1,21}[a-z0-9]$'),
  endpoint_id text REFERENCES endpoints ON DELETE SET NULL,
  owner_id    uuid REFERENCES users ON DELETE SET NULL,
  created_at  timestamptz NOT NULL DEFAULT now(),
  released_at timestamptz
);
CREATE INDEX endpoint_aliases_endpoint ON endpoint_aliases (endpoint_id) WHERE endpoint_id IS NOT NULL;
CREATE INDEX endpoint_aliases_owner ON endpoint_aliases (owner_id) WHERE released_at IS NULL;

CREATE FUNCTION release_endpoint_aliases() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  UPDATE endpoint_aliases SET released_at = now(), endpoint_id = NULL
  WHERE endpoint_id = OLD.id AND released_at IS NULL;
  IF TG_OP = 'DELETE' THEN
    RETURN OLD;
  END IF;
  RETURN NEW;
END;
$$;

CREATE TRIGGER endpoints_release_aliases_on_delete
  BEFORE DELETE ON endpoints
  FOR EACH ROW EXECUTE FUNCTION release_endpoint_aliases();

CREATE TRIGGER endpoints_release_aliases_on_soft_delete
  AFTER UPDATE OF deleted_at ON endpoints
  FOR EACH ROW WHEN (OLD.deleted_at IS NULL AND NEW.deleted_at IS NOT NULL)
  EXECUTE FUNCTION release_endpoint_aliases();
