DROP TABLE endpoint_vars;

ALTER TABLE endpoints
  DROP COLUMN response_status_template,
  DROP COLUMN response_templated;
