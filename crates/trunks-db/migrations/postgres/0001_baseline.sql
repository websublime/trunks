-- Baseline schema. Idempotent: a database created by the legacy gateway already has these
-- tables, so every statement is IF NOT EXISTS. Column names are part of the data contract (D3).
CREATE TABLE IF NOT EXISTS mcp_servers (
  id BIGSERIAL PRIMARY KEY,
  created_at TIMESTAMPTZ, updated_at TIMESTAMPTZ, deleted_at TIMESTAMPTZ,
  name TEXT NOT NULL,
  transport TEXT NOT NULL,
  enabled BOOLEAN NOT NULL DEFAULT true,
  description TEXT,
  config JSONB NOT NULL,
  session_mode TEXT DEFAULT 'stateless'
);
CREATE INDEX IF NOT EXISTS idx_mcp_servers_deleted_at ON mcp_servers(deleted_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_servers_name ON mcp_servers(name);

CREATE TABLE IF NOT EXISTS tools (
  id BIGSERIAL PRIMARY KEY,
  created_at TIMESTAMPTZ, updated_at TIMESTAMPTZ, deleted_at TIMESTAMPTZ,
  name TEXT NOT NULL,
  enabled BOOLEAN NOT NULL DEFAULT true,
  description TEXT,
  input_schema JSONB,
  annotations JSONB,
  server_id BIGINT NOT NULL,
  CONSTRAINT fk_tools_server FOREIGN KEY (server_id) REFERENCES mcp_servers(id)
);
CREATE INDEX IF NOT EXISTS idx_tools_deleted_at ON tools(deleted_at);

CREATE TABLE IF NOT EXISTS server_configs (
  id BIGSERIAL PRIMARY KEY,
  created_at TIMESTAMPTZ, updated_at TIMESTAMPTZ, deleted_at TIMESTAMPTZ,
  mode TEXT NOT NULL,
  initialized BOOLEAN NOT NULL DEFAULT false
);
CREATE INDEX IF NOT EXISTS idx_server_configs_deleted_at ON server_configs(deleted_at);

CREATE TABLE IF NOT EXISTS users (
  id BIGSERIAL PRIMARY KEY,
  created_at TIMESTAMPTZ, updated_at TIMESTAMPTZ, deleted_at TIMESTAMPTZ,
  username TEXT NOT NULL UNIQUE,
  role TEXT NOT NULL,
  access_token TEXT NOT NULL UNIQUE
);
CREATE INDEX IF NOT EXISTS idx_users_deleted_at ON users(deleted_at);

CREATE TABLE IF NOT EXISTS mcp_clients (
  id BIGSERIAL PRIMARY KEY,
  created_at TIMESTAMPTZ, updated_at TIMESTAMPTZ, deleted_at TIMESTAMPTZ,
  name TEXT NOT NULL,
  description TEXT,
  access_token TEXT NOT NULL UNIQUE,
  allow_list JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mcp_clients_deleted_at ON mcp_clients(deleted_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_clients_name ON mcp_clients(name);

CREATE TABLE IF NOT EXISTS tool_groups (
  id BIGSERIAL PRIMARY KEY,
  created_at TIMESTAMPTZ, updated_at TIMESTAMPTZ, deleted_at TIMESTAMPTZ,
  name TEXT NOT NULL UNIQUE,
  description TEXT,
  included_tools JSONB,
  included_servers JSONB,
  excluded_tools JSONB
);
CREATE INDEX IF NOT EXISTS idx_tool_groups_deleted_at ON tool_groups(deleted_at);

CREATE TABLE IF NOT EXISTS prompts (
  id BIGSERIAL PRIMARY KEY,
  created_at TIMESTAMPTZ, updated_at TIMESTAMPTZ, deleted_at TIMESTAMPTZ,
  name TEXT NOT NULL,
  enabled BOOLEAN NOT NULL DEFAULT true,
  description TEXT,
  arguments JSONB,
  server_id BIGINT NOT NULL,
  CONSTRAINT fk_prompts_server FOREIGN KEY (server_id) REFERENCES mcp_servers(id)
);
CREATE INDEX IF NOT EXISTS idx_prompts_deleted_at ON prompts(deleted_at);

CREATE TABLE IF NOT EXISTS resources (
  id BIGSERIAL PRIMARY KEY,
  created_at TIMESTAMPTZ, updated_at TIMESTAMPTZ, deleted_at TIMESTAMPTZ,
  uri TEXT NOT NULL,
  original_uri TEXT NOT NULL,
  name TEXT NOT NULL,
  enabled BOOLEAN NOT NULL DEFAULT true,
  description TEXT,
  mime_type TEXT,
  annotations JSONB,
  meta JSONB,
  server_id BIGINT NOT NULL,
  CONSTRAINT fk_resources_server FOREIGN KEY (server_id) REFERENCES mcp_servers(id)
);
CREATE INDEX IF NOT EXISTS idx_resources_deleted_at ON resources(deleted_at);

CREATE TABLE IF NOT EXISTS upstream_oauth_pending_sessions (
  id BIGSERIAL PRIMARY KEY,
  created_at TIMESTAMPTZ, updated_at TIMESTAMPTZ, deleted_at TIMESTAMPTZ,
  session_id TEXT NOT NULL,
  server_name TEXT NOT NULL,
  transport TEXT NOT NULL,
  server_input JSONB NOT NULL,
  force BOOLEAN NOT NULL DEFAULT false,
  redirect_uri TEXT, client_id TEXT, client_secret TEXT,
  scopes JSONB,
  state TEXT NOT NULL,
  code_verifier TEXT NOT NULL,
  expires_at TIMESTAMPTZ NOT NULL,
  initiated_by TEXT
);
CREATE INDEX IF NOT EXISTS idx_upstream_oauth_pending_sessions_deleted_at ON upstream_oauth_pending_sessions(deleted_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_upstream_oauth_pending_sessions_session_id ON upstream_oauth_pending_sessions(session_id);
CREATE INDEX IF NOT EXISTS idx_upstream_oauth_pending_sessions_server_name ON upstream_oauth_pending_sessions(server_name);
CREATE INDEX IF NOT EXISTS idx_upstream_oauth_pending_sessions_expires_at ON upstream_oauth_pending_sessions(expires_at);

CREATE TABLE IF NOT EXISTS upstream_oauth_tokens (
  id BIGSERIAL PRIMARY KEY,
  created_at TIMESTAMPTZ, updated_at TIMESTAMPTZ, deleted_at TIMESTAMPTZ,
  server_name TEXT NOT NULL,
  transport TEXT NOT NULL,
  client_id TEXT, client_secret TEXT, redirect_uri TEXT,
  scopes JSONB,
  access_token TEXT, token_type TEXT, refresh_token TEXT, scope TEXT,
  expires_at TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_upstream_oauth_tokens_deleted_at ON upstream_oauth_tokens(deleted_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_upstream_oauth_tokens_server_name ON upstream_oauth_tokens(server_name);
