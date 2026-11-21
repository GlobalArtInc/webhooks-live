ALTER TABLE login_states
  ADD COLUMN client_net cidr;

CREATE INDEX login_states_client_net ON login_states (client_net) WHERE client_net IS NOT NULL;
