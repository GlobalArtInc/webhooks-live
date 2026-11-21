DROP INDEX login_states_expires;

ALTER TABLE users
  DROP COLUMN email_verified;
