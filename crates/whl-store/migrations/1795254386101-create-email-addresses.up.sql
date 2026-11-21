CREATE TABLE email_addresses (
  id                uuid PRIMARY KEY,
  user_id           uuid NOT NULL REFERENCES users ON DELETE CASCADE,
  address           text NOT NULL CHECK (char_length(address) BETWEEN 3 AND 254),
  verified_at       timestamptz,
  idp_verified      boolean NOT NULL DEFAULT false,
  verify_token_hash bytea CHECK (verify_token_hash IS NULL OR octet_length(verify_token_hash) = 32),
  verify_expires_at timestamptz,
  created_at        timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT email_addresses_token_complete CHECK (
    (verify_token_hash IS NULL) = (verify_expires_at IS NULL)
  )
);
CREATE UNIQUE INDEX email_addresses_user_address ON email_addresses (user_id, lower(address));
CREATE UNIQUE INDEX email_addresses_verify_token ON email_addresses (verify_token_hash)
  WHERE verify_token_hash IS NOT NULL;
