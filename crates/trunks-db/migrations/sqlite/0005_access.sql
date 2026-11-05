-- 0005: teams, memberships, resource-side grants, MCP client ownership (D14).
CREATE TABLE IF NOT EXISTS teams (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at TEXT, updated_at TEXT, deleted_at TEXT,
  name TEXT NOT NULL,
  description TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_teams_name ON teams(name);

CREATE TABLE IF NOT EXISTS team_members (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at TEXT, updated_at TEXT,
  team_id INTEGER NOT NULL,
  user_id INTEGER NOT NULL,
  role TEXT NOT NULL DEFAULT 'member'
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_team_members_team_user ON team_members(team_id, user_id);
CREATE INDEX IF NOT EXISTS idx_team_members_user ON team_members(user_id);

CREATE TABLE IF NOT EXISTS access_grants (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at TEXT, updated_at TEXT,
  subject_kind TEXT NOT NULL,
  subject_id INTEGER NOT NULL,
  target_kind TEXT NOT NULL,
  target_name TEXT NOT NULL,
  permission TEXT NOT NULL DEFAULT 'use'
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_access_grants_unique ON access_grants(subject_kind, subject_id, target_kind, target_name);
CREATE INDEX IF NOT EXISTS idx_access_grants_target ON access_grants(target_kind, target_name);

ALTER TABLE mcp_clients ADD COLUMN owner_kind TEXT;
ALTER TABLE mcp_clients ADD COLUMN owner_id INTEGER;
