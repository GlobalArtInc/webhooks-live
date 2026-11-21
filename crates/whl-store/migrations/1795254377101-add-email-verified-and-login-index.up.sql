ALTER TABLE users
  ADD COLUMN email_verified boolean NOT NULL DEFAULT false;

CREATE INDEX login_states_expires ON login_states (expires_at);
