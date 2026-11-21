ALTER TABLE endpoints ADD COLUMN verifier jsonb;
ALTER TABLE captures
  ADD COLUMN verification text CHECK (verification IN ('valid', 'invalid', 'missing'));
