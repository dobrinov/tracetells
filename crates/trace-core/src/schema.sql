-- Plan templates. A null limit means no limit.
CREATE TABLE IF NOT EXISTS plans (
  id INTEGER PRIMARY KEY,
  key TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  retention_days INTEGER,
  max_sites INTEGER,
  monthly_events INTEGER,
  max_members INTEGER,
  monthly_ai_runs INTEGER,
  is_default INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);

-- An account (a company or one person) owns websites, people, tokens, the
-- activity log and a plan. `overrides` is a JSON object: a key present
-- overrides that limit for this account only (null meaning no limit).
CREATE TABLE IF NOT EXISTS accounts (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  plan_id INTEGER NOT NULL REFERENCES plans(id),
  overrides TEXT NOT NULL DEFAULT '{}',
  -- Every person must use two-step verification.
  require_mfa INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);

-- Monthly usage per account (month is YYYY-MM, UTC).
CREATE TABLE IF NOT EXISTS usage (
  account_id INTEGER NOT NULL,
  month TEXT NOT NULL,
  events INTEGER NOT NULL DEFAULT 0,
  events_dropped INTEGER NOT NULL DEFAULT 0,
  ai_runs INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (account_id, month)
);

-- A project is one website.
CREATE TABLE IF NOT EXISTS projects (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  write_key TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL,
  -- The last trace.json applied, and where it came from.
  config TEXT,
  config_synced_at INTEGER,
  config_source TEXT,
  -- When the collector last delivered events (not seeded or imported data).
  last_event_at INTEGER,
  account_id INTEGER REFERENCES accounts(id),
  -- Short identifier, unique within the account, used in URLs and --site.
  slug TEXT,
  -- When someone confirmed the setup checklist's privacy review, and who.
  privacy_reviewed_at INTEGER,
  privacy_reviewed_by TEXT
);

-- The sites (origins) the collector is sending from: the dashboard's proof
-- that collection works, and where.
CREATE TABLE IF NOT EXISTS event_sources (
  project_id INTEGER NOT NULL,
  origin TEXT NOT NULL,
  first_seen INTEGER NOT NULL,
  last_seen INTEGER NOT NULL,
  PRIMARY KEY (project_id, origin)
);

CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  project_id INTEGER NOT NULL REFERENCES projects(id),
  visitor_id TEXT,
  user_id TEXT,
  started_at INTEGER NOT NULL,
  last_seen_at INTEGER NOT NULL,
  device TEXT,
  user_agent TEXT,
  entry_path TEXT,
  referrer TEXT
);
CREATE INDEX IF NOT EXISTS sessions_project_started ON sessions(project_id, started_at);

-- One row per captured interaction. `selector` holds the element selector for
-- clicks and forms, and the source location for errors. `value` holds numeric
-- payloads: scroll depth (%) for `scroll`, time on page (ms) for `leave`.
CREATE TABLE IF NOT EXISTS events (
  id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL,
  session_id TEXT NOT NULL,
  ts INTEGER NOT NULL,
  type TEXT NOT NULL,
  path TEXT,
  selector TEXT,
  text TEXT,
  name TEXT,
  value REAL,
  props TEXT
);
CREATE INDEX IF NOT EXISTS events_project_type_ts ON events(project_id, type, ts);
CREATE INDEX IF NOT EXISTS events_session_ts ON events(session_id, ts);

-- KPIs are defined in the host application's trace.json and synced here,
-- matched by `key`. KPIs removed from the file are archived, not deleted, so
-- insights and evaluations that point at them keep working.
CREATE TABLE IF NOT EXISTS kpis (
  id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL,
  key TEXT,
  archived_at INTEGER,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  definition TEXT NOT NULL,
  direction TEXT NOT NULL,
  target REAL,
  created_at INTEGER NOT NULL
);

-- The custom events the host application declares in trace.json.
CREATE TABLE IF NOT EXISTS event_definitions (
  project_id INTEGER NOT NULL,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  PRIMARY KEY (project_id, name)
);

CREATE TABLE IF NOT EXISTS insights (
  id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL,
  title TEXT NOT NULL,
  problem TEXT NOT NULL,
  evidence TEXT NOT NULL DEFAULT '[]',
  proposal TEXT NOT NULL,
  acceptance_criteria TEXT NOT NULL DEFAULT '[]',
  pages TEXT NOT NULL DEFAULT '[]',
  kpi_id INTEGER REFERENCES kpis(id) ON DELETE SET NULL,
  expected_impact TEXT NOT NULL DEFAULT '',
  confidence TEXT NOT NULL DEFAULT 'medium',
  status TEXT NOT NULL DEFAULT 'new',
  dismiss_reason TEXT,
  source TEXT NOT NULL,
  fingerprint TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  released_at INTEGER
);
CREATE INDEX IF NOT EXISTS insights_project_status ON insights(project_id, status);

-- Latest before/after measurement of a released insight against its KPI.
CREATE TABLE IF NOT EXISTS evaluations (
  insight_id INTEGER PRIMARY KEY REFERENCES insights(id) ON DELETE CASCADE,
  kpi_id INTEGER NOT NULL,
  baseline REAL,
  current REAL,
  baseline_n INTEGER NOT NULL,
  current_n INTEGER NOT NULL,
  p_value REAL,
  verdict TEXT NOT NULL,
  summary TEXT NOT NULL,
  window_days REAL NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS analysis_runs (
  id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL,
  engine TEXT NOT NULL,
  model TEXT,
  started_at INTEGER NOT NULL,
  finished_at INTEGER,
  insights_created INTEGER NOT NULL DEFAULT 0,
  error TEXT
);

-- People who use the dashboard. Passwords are Argon2id hashes. TOTP secrets
-- are stored as-is (they must be readable to check codes); everything the
-- server hands out (sessions, invites, API tokens, recovery codes) is stored
-- only as a SHA-256 hash.
CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL,
  email TEXT NOT NULL,
  name TEXT NOT NULL DEFAULT '',
  password_hash TEXT NOT NULL,
  role TEXT NOT NULL,
  totp_secret TEXT,
  totp_pending_secret TEXT,
  totp_enabled_at INTEGER,
  totp_last_step INTEGER,
  created_at INTEGER NOT NULL,
  last_login_at INTEGER,
  -- Operator rights (the internal admin area). Only `trace operator` sets it.
  is_operator INTEGER NOT NULL DEFAULT 0,
  -- An email identifies one person on the whole server.
  UNIQUE (email)
);

CREATE TABLE IF NOT EXISTS recovery_codes (
  user_id INTEGER NOT NULL,
  code_hash TEXT NOT NULL,
  used_at INTEGER,
  PRIMARY KEY (user_id, code_hash)
);

CREATE TABLE IF NOT EXISTS auth_sessions (
  token_hash TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  last_seen_at INTEGER NOT NULL,
  -- 0 while a second factor is still due.
  mfa_ok INTEGER NOT NULL,
  user_agent TEXT
);
CREATE INDEX IF NOT EXISTS auth_sessions_user ON auth_sessions(user_id);

CREATE TABLE IF NOT EXISTS invites (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL,
  token_hash TEXT NOT NULL UNIQUE,
  email TEXT NOT NULL,
  role TEXT NOT NULL,
  invited_by INTEGER,
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  accepted_at INTEGER
);

CREATE TABLE IF NOT EXISTS api_tokens (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL,
  name TEXT NOT NULL,
  token_hash TEXT NOT NULL UNIQUE,
  prefix TEXT NOT NULL,
  -- The person the token belongs to; it acts with their current role.
  created_by INTEGER,
  -- Can read what the owner can, but never change anything.
  read_only INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  last_used_at INTEGER,
  revoked_at INTEGER
);

-- Who changed what: team, security settings, API tokens, privacy rules.
CREATE TABLE IF NOT EXISTS audit_log (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL,
  -- The website an entry is about, if any.
  project_id INTEGER,
  actor TEXT NOT NULL,
  action TEXT NOT NULL,
  detail TEXT NOT NULL DEFAULT '',
  at INTEGER NOT NULL
);

-- Privacy rules an admin adds on top of the built-in ones, managed in the dashboard.
-- kind: mask_selector | redact_pattern | ignore_path | drop_prop
CREATE TABLE IF NOT EXISTS privacy_rules (
  id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL,
  kind TEXT NOT NULL,
  pattern TEXT NOT NULL,
  replacement TEXT,
  label TEXT NOT NULL DEFAULT '',
  enabled INTEGER NOT NULL DEFAULT 1,
  created_by TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
