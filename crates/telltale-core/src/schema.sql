CREATE TABLE IF NOT EXISTS projects (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  write_key TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL
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

CREATE TABLE IF NOT EXISTS kpis (
  id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  definition TEXT NOT NULL,
  direction TEXT NOT NULL,
  target REAL,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS ideas (
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
CREATE INDEX IF NOT EXISTS ideas_project_status ON ideas(project_id, status);

-- Latest before/after measurement of a released idea against its KPI.
CREATE TABLE IF NOT EXISTS evaluations (
  idea_id INTEGER PRIMARY KEY REFERENCES ideas(id) ON DELETE CASCADE,
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
  ideas_created INTEGER NOT NULL DEFAULT 0,
  error TEXT
);
