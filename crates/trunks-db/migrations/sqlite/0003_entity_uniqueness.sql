-- One tool/prompt per (server, name) and one resource per URI among live rows.
-- Keep the newest row where legacy data has duplicates, then enforce with partial unique indexes.
DELETE FROM tools WHERE deleted_at IS NULL AND id NOT IN (SELECT MAX(id) FROM tools WHERE deleted_at IS NULL GROUP BY server_id, name);
DELETE FROM prompts WHERE deleted_at IS NULL AND id NOT IN (SELECT MAX(id) FROM prompts WHERE deleted_at IS NULL GROUP BY server_id, name);
DELETE FROM resources WHERE deleted_at IS NULL AND id NOT IN (SELECT MAX(id) FROM resources WHERE deleted_at IS NULL GROUP BY uri);
CREATE UNIQUE INDEX IF NOT EXISTS ux_tools_server_name ON tools(server_id, name) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS ux_prompts_server_name ON prompts(server_id, name) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS ux_resources_uri ON resources(uri) WHERE deleted_at IS NULL;
