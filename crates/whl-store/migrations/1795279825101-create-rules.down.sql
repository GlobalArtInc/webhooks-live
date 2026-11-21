DROP TABLE rules;

ALTER TABLE endpoints
  DROP COLUMN rules_sync,
  DROP COLUMN rule_count,
  DROP COLUMN rules_version;
