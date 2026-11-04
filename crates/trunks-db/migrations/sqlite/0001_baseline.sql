-- Baseline schema. Idempotent: a database created by the legacy gateway already has these
-- tables, so every statement is IF NOT EXISTS. Column names are part of the data contract (D3).
CREATE TABLE IF NOT EXISTS mcp_servers (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at TEXT, updated_at TEXT, deleted_at TEXT,
  name TEXT NOT NULL,
  transport TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  description TEXT,
  config TEXT NOT NULL,
  session_mode TEXT DEFAULT 'stateless'
);
CREATE INDEX IF NOT EXISTS idx_mcp_servers_deleted_at ON mcp_servers(deleted_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_servers_name ON mcp_servers(name);

CREATE TABLE IF NOT EXISTS tools (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at TEXT, updated_at TEXT, deleted_at TEXT,
  name TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  description TEXT,
  input_schema TEXT,
  annotations TEXT,
  server_id INTEGER NOT NULL,
  CONSTRAINT fk_tools_server FOREIGN KEY (server_id) REFERENCES mcp_servers(id)
);
CREATE INDEX IF NOT EXISTS idx_tools_deleted_at ON tools(deleted_at);

CREATE TABLE IF NOT EXISTS server_configs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at TEXT, updated_at TEXT, deleted_at TEXT,
  mode TEXT NOT NULL,
  initialized INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_server_configs_deleted_at ON server_configs(deleted_at);

CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at TEXT, updated_at TEXT, deleted_at TEXT,
  username TEXT NOT NULL UNIQUE,
  role TEXT NOT NULL,
  access_token TEXT NOT NULL UNIQUE
);
CREATE INDEX IF NOT EXISTS idx_users_deleted_at ON users(deleted_at);

CREATE TABLE IF NOT EXISTS mcp_clients (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at TEXT, updated_at TEXT, deleted_at TEXT,
  name TEXT NOT NULL,
  description TEXT,
  access_token TEXT NOT NULL UNIQUE,
  allow_list TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mcp_clients_deleted_at ON mcp_clients(deleted_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_clients_name ON mcp_clients(name);

CREATE TABLE IF NOT EXISTS tool_groups (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at TEXT, updated_at TEXT, deleted_at TEXT,
  name TEXT NOT NULL UNIQUE,
  description TEXT,
  included_tools TEXT,
  included_servers TEXT,
  excluded_tools TEXT
);
CREATE INDEX IF NOT EXISTS idx_tool_groups_deleted_at ON tool_groups(deleted_at);

CREATE TABLE IF NOT EXISTS prompts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at TEXT, updated_at TEXT, deleted_at TEXT,
  name TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  description TEXT,
  arguments TEXT,
  server_id INTEGER NOT NULL,
  CONSTRAINT fk_prompts_server FOREIGN KEY (server_id) REFERENCES mcp_servers(id)
);
CREATE INDEX IF NOT EXISTS idx_prompts_deleted_at ON prompts(deleted_at);

CREATE TABLE IF NOT EXISTS resources (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at TEXT, updated_at TEXT, deleted_at TEXT,
  uri TEXT NOT NULL,
  original_uri TEXT NOT NULL,
  name TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  description TEXT,
  mime_type TEXT,
  annotations TEXT,
  meta TEXT,
  server_id INTEGER NOT NULL,
  CONSTRAINT fk_resources_server FOREIGN KEY (server_id) REFERENCES mcp_servers(id)
);
CREATE INDEX IF NOT EXISTS idx_resources_deleted_at ON resources(deleted_at);

CREATE TABLE IF NOT EXISTS upstream_oauth_pending_sessions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at TEXT, updated_at TEXT, deleted_at TEXT,
  session_id TEXT NOT NULL,
  server_name TEXT NOT NULL,
  transport TEXT NOT NULL,
  server_input TEXT NOT NULL,
  force INTEGER NOT NULL DEFAULT 0,
  redirect_uri TEXT, client_id TEXT, client_secret TEXT,
  scopes TEXT,
  state TEXT NOT NULL,
  code_verifier TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  initiated_by TEXT
);
CREATE INDEX IF NOT EXISTS idx_upstream_oauth_pending_sessions_deleted_at ON upstream_oauth_pending_sessions(deleted_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_upstream_oauth_pending_sessions_session_id ON upstream_oauth_pending_sessions(session_id);
CREATE INDEX IF NOT EXISTS idx_upstream_oauth_pending_sessions_server_name ON upstream_oauth_pending_sessions(server_name);
CREATE INDEX IF NOT EXISTS idx_upstream_oauth_pending_sessions_expires_at ON upstream_oauth_pending_sessions(expires_at);

CREATE TABLE IF NOT EXISTS upstream_oauth_tokens (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at TEXT, updated_at TEXT, deleted_at TEXT,
  server_name TEXT NOT NULL,
  transport TEXT NOT NULL,
  client_id TEXT, client_secret TEXT, redirect_uri TEXT,
  scopes TEXT,
  access_token TEXT, token_type TEXT, refresh_token TEXT, scope TEXT,
  expires_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_upstream_oauth_tokens_deleted_at ON upstream_oauth_tokens(deleted_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_upstream_oauth_tokens_server_name ON upstream_oauth_tokens(server_name);
