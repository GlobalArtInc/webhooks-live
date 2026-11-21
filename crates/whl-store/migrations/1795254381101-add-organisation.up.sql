ALTER TABLE endpoints
  ADD COLUMN tags text[] NOT NULL DEFAULT '{}' CHECK (cardinality(tags) <= 10);
CREATE INDEX endpoints_tags ON endpoints USING gin (tags);

ALTER TABLE captures
  ADD COLUMN note text CHECK (char_length(note) <= 2000),
  ADD COLUMN starred boolean NOT NULL DEFAULT false;

CREATE TABLE folders (
  id         uuid PRIMARY KEY,
  user_id    uuid NOT NULL REFERENCES users ON DELETE CASCADE,
  name       text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 64),
  position   integer NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (user_id, name)
);

CREATE TABLE folder_endpoints (
  folder_id   uuid NOT NULL REFERENCES folders ON DELETE CASCADE,
  endpoint_id text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  added_at    timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (folder_id, endpoint_id)
);
CREATE INDEX folder_endpoints_endpoint ON folder_endpoints (endpoint_id);
