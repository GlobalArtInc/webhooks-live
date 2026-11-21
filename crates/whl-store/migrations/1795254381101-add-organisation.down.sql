DROP TABLE folder_endpoints;
DROP TABLE folders;

ALTER TABLE captures
  DROP COLUMN starred,
  DROP COLUMN note;

DROP INDEX endpoints_tags;
ALTER TABLE endpoints
  DROP COLUMN tags;
