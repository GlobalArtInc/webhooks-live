DROP TRIGGER endpoints_release_aliases_on_soft_delete ON endpoints;
DROP TRIGGER endpoints_release_aliases_on_delete ON endpoints;
DROP FUNCTION release_endpoint_aliases();
DROP TABLE endpoint_aliases;
