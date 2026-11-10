-- 0006: OIDC login for humans (D16/D17): external identity on users, browser sessions,
-- in-flight login states, and the origin of a team membership (manual vs synced from claims).
ALTER TABLE users ADD COLUMN IF NOT EXISTS oidc_issuer TEXT;
ALTER TABLE users ADD COLUMN IF NOT EXISTS oidc_subject TEXT;
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_oidc ON users(oidc_issuer, oidc_subject) WHERE oidc_subject IS NOT NULL;

ALTER TABLE team_members ADD COLUMN IF NOT EXISTS source TEXT NOT NULL DEFAULT 'manual';

CREATE TABLE IF NOT EXISTS sessions (
  id_hash TEXT PRIMARY KEY,
  user_id BIGINT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL,
  expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_expires ON sessions(expires_at);

CREATE TABLE IF NOT EXISTS oidc_login_states (
  state TEXT PRIMARY KEY,
  nonce TEXT NOT NULL,
  code_verifier TEXT NOT NULL,
  return_to TEXT NOT NULL,
  expires_at TIMESTAMPTZ NOT NULL
);
