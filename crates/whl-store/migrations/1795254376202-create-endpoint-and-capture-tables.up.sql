CREATE TABLE endpoints (
  id                    text PRIMARY KEY CHECK (id ~ '^[0-9a-hjkmnp-tv-z]{24}$'),
  owner_id              uuid REFERENCES users,
  title                 text,
  response_status       smallint NOT NULL DEFAULT 200 CHECK (response_status BETWEEN 100 AND 599),
  response_content_type text NOT NULL DEFAULT 'text/plain',
  response_headers      jsonb NOT NULL DEFAULT '[]',
  response_body         bytea NOT NULL DEFAULT '',
  response_delay_ms     integer NOT NULL DEFAULT 0 CHECK (response_delay_ms BETWEEN 0 AND 10000),
  cors                  boolean NOT NULL DEFAULT false,
  overflow              text NOT NULL DEFAULT 'refuse' CHECK (overflow IN ('refuse', 'ring')),
  capture_count         integer NOT NULL DEFAULT 0,
  bytes_used            bigint NOT NULL DEFAULT 0,
  last_seq              bigint NOT NULL DEFAULT 0,
  created_at            timestamptz NOT NULL DEFAULT now(),
  last_active_at        timestamptz NOT NULL DEFAULT now(),
  deleted_at            timestamptz
) WITH (fillfactor = 80);
CREATE INDEX endpoints_owner ON endpoints (owner_id) WHERE owner_id IS NOT NULL;
CREATE INDEX endpoints_deleted ON endpoints (deleted_at) WHERE deleted_at IS NOT NULL;

CREATE TABLE endpoint_members (
  endpoint_id text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  user_id     uuid NOT NULL REFERENCES users ON DELETE CASCADE,
  role        text NOT NULL CHECK (role IN ('viewer', 'editor')),
  created_at  timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (endpoint_id, user_id)
);
CREATE INDEX endpoint_members_user ON endpoint_members (user_id);

CREATE TABLE captures (
  endpoint_id     text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  seq             bigint NOT NULL,
  id              uuid NOT NULL UNIQUE,
  received_at     timestamptz NOT NULL,
  method          text NOT NULL,
  url             text NOT NULL,
  path            text NOT NULL,
  host            text NOT NULL,
  client_ip       inet NOT NULL,
  headers         jsonb NOT NULL,
  query           jsonb NOT NULL,
  form            jsonb,
  content_type    text,
  body_size       integer NOT NULL,
  stored_bytes    integer NOT NULL,
  status_override smallint,
  response_status smallint NOT NULL,
  PRIMARY KEY (endpoint_id, seq)
);

CREATE TABLE capture_bodies (
  capture_id  uuid PRIMARY KEY REFERENCES captures (id) ON DELETE CASCADE,
  data        bytea COMPRESSION lz4 NOT NULL,
  search_text text COMPRESSION lz4 NOT NULL
);
