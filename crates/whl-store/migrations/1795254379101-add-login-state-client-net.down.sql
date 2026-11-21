DROP INDEX login_states_client_net;

ALTER TABLE login_states
  DROP COLUMN client_net;
