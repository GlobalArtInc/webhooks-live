DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM endpoints WHERE view_password_hash IS NOT NULL) THEN
    RAISE EXCEPTION 'cannot roll back: endpoints still have view passwords and would become public';
  END IF;
END
$$;

DROP TABLE endpoint_grants;

ALTER TABLE endpoints
  DROP CONSTRAINT endpoints_view_password_hash_size,
  DROP CONSTRAINT endpoints_view_password_needs_no_owner,
  DROP COLUMN view_password_hash;
