-- Normalise rows written by legacy deployments (D3). Idempotent.
UPDATE resources SET uri = replace(uri, 'mcpj://', 'trunks://') WHERE uri LIKE 'mcpj://%';
UPDATE server_configs SET mode = 'enterprise' WHERE mode = 'production';
UPDATE mcp_servers SET session_mode = 'stateless' WHERE session_mode IS NULL OR session_mode = '';
