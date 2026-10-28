# Design

## Context

- **Measuring today** (`crates/trace-core/src/kpi.rs`).
  - `kpi::measure(pool, site, &KpiDefinition, from, to)` evaluates the three KPI kinds in `model.rs`:
    - `Conversion { from, to }`, `SessionRate { matcher }`, `PerSession { matcher }`;
    - `EventMatch { kind, path, selector, name }`, where patterns ending in `*` match by prefix (`pattern_matches`).
  - It returns a `Measurement { value, samples }`, one sample per session, which `evaluate::welch` uses.
  - `hits()` loads **every** event of the type in the window and filters path, selector and name in Rust. That is fine for KPIs (a handful, hourly), but too slow for ad-hoc questions with breakdowns on a busy site.
- **What agents read** (`crates/trace-core/src/digest.rs`).
  - `digest::build` produces the fixed summary that `get_behaviour_summary` returns: pages, transitions, clicks, errors, forms, custom events, KPIs.
  - Its visitor text is cleaned again (`visitor_text_on_one_line`).
- **Insights** (`crates/trace-core/src/insights.rs`, `model.rs`).
  - `insights` rows hold `evidence` as a JSON array of strings, `source` (`heuristics`, `claude`, `agent`, `demo`), `fingerprint`, `created_at`, `released_at`, `released_definition` and `released_direction`. There is no record of who created an insight.
  - `insights::insert` skips duplicates by fingerprint or title.
- **Agents saving insights** (`crates/trace-server/src/mcp.rs`).
  - `add_insights` validates with `agent_insight()` (lengths, KPI key, confidence) and inserts directly, with `source: "agent"` and the fingerprint prefixed `agent:`.
  - `analyse_website` is the analysis prompt.
  - `Context` carries the `actor` string (for example `mcp: a@x via token "Claude Code (signed in …)"`), `role`, `read_only`, `origin` and `state`.
- **Server-side analysis** (`crates/trace-agent/src/heuristics.rs`, `analyst.rs`). Both build `NewInsight` with string evidence such as `"12 rage-click bursts from 9 sessions in the last 14 days"`.
- **Evaluation** (`crates/trace-core/src/evaluate.rs`). It judges a released insight's KPI over fixed 14-day windows, then settles (`evaluations.settled_at`). It runs in the server's hourly loop (`main.rs`, next to `retention::purge`).
- **Surfaces.**
  - Dashboard (`crates/trace-server/assets/app.js`): `insightCard` shows evidence as a list of strings, and `verdictBlock` shows the KPI verdict.
  - Public view (`crates/trace-server/src/public.rs`): `insight_json` allowlists insight fields.
  - Prompt (`crates/trace-core/src/prompt.rs`): renders evidence as bullets.

## Goals / Non-Goals

**Goals:**
- Agents can ask precise aggregated questions cheaply.
- Every number in an insight comes from Trace, and keeps being measured.
- Nothing is saved without the person seeing it first, and every insight says who made it, when, and from what.
- After a release, the evidence shows whether the specific problem went away, next to the KPI verdict.

**Non-Goals:**
- Per-session or per-person views, exports, or raw event access.
- A query language beyond the KPI matchers (no arbitrary SQL, no joins across websites).
- Editing an insight's text when it's refreshed. Text stays what the person approved, and only the numbers move.
- Server-side AI. The analysis stays in the person's agent, and the optional Claude analyst is unchanged apart from metric specs.

## Decisions

### D1. One metric spec for queries, evidence and KPIs
`query::Metric` describes a single measurement: a `measure` plus one or two matchers, using the same `EventMatch`.

```json
{ "measure": "session_rate", "match": { "type": "rage_click", "path": "/checkout", "selector": "button#pay" } }
{ "measure": "conversion", "from": { "pageview": "/checkout" }, "to": { "event": "purchase" } }
```

- **Measures:** `sessions` (sessions with a match), `events` (matching events), `session_rate`, `per_session` and `conversion`.
- **Shorthand:** the parser accepts the trace.json matcher shorthands (`{ "pageview": "/checkout" }`), via the same code as `config.rs`.
- **Mapping to KPIs:** `session_rate`, `per_session` and `conversion` map one to one onto `KpiDefinition`, so a measurement can be judged with `evaluate::welch` exactly like a KPI.
- **Why one shape:** agents learn it once, and evidence, queries and KPIs can't drift apart.

### D2. Measuring in SQL
`query::measure(pool, site, &Metric, window, breakdown)` pushes the matcher into SQL:
- `type = $`, then `path = $` or `path LIKE $ || '%'` for a trailing `*`, and the same for selector and name;
- `COUNT(DISTINCT session_id)` for session-based measures;
- `GROUP BY` for breakdowns.

Specifics:
- **Breakdown by device** joins `sessions` for `device`.
- **Breakdown by day** groups by `ts / 86400000`, which works the same in SQLite and Postgres.
- **Conversion** needs order (to after from), so it runs as two grouped queries: the first `from` per session, and the `to` timestamps for those sessions. Then it joins in Rust over session ids only, which is bounded by the sessions in the window.
- **KPIs** keep `kpi::measure` for now, because it also needs per-session samples for the Welch test. The SQL path returns samples too, when asked for them.
- **Wildcards:** `LIKE` escapes `%` and `_` in the pattern.
- **Limits:**
  - a query runs at most 3 statements and returns at most 50 rows;
  - the window is 1 to 90 days, which is also capped by retention;
  - matcher text is at most 200 characters;
  - rows from fewer than 3 sessions are merged into "other".
- **Rate limits:** `query_behaviour` and `draft_insights` go through the existing `guard::Counter`: 60 per token per minute.
- **Indexes:** the existing `events (project_id, ts)` index covers the window. A `(project_id, type, ts)` index is added in `db::migrate` for the type filter.

### D3. Evidence items
`model::Evidence` is one of:
- `Measured { claim, metric, goal: "decrease"|"increase" }`, where `goal` says which way the evidence should move if the insight is right (rage clicks: decrease);
- `Note { text }`.

Storage:
- `insights.evidence` stays a JSON column, now holding `[{ "claim", "metric", "goal" } | { "note" }]`.
- Rows where it's a plain array of strings are read as notes, so old insights load unchanged without a data migration.
- The first measurement goes into `insight_measurements` (D5).
- `claim` is the agent's wording and is shown with Trace's number next to it, as in "Most checkout visitors rage-click Pay now: 18.2% of 410 sessions, 14–28 Oct". The agent never supplies the number.

### D4. Drafts
- **Table:** `insight_drafts (id TEXT PRIMARY KEY, project_id, created_by, payload TEXT, measured TEXT, created_at)`.
  - `id` is random.
  - `payload` holds the validated `NewInsight`, and `measured` holds the measurements taken at drafting.
  - Drafts older than an hour are deleted whenever drafts are written.
- **`draft_insights`** (any token, read-only too, so a read-only agent can still show you what it would save). It takes 1 to 10 insights:
  - validates them with an extended `agent_insight()`;
  - measures every metric;
  - refuses an insight with no measured item, or whose metrics all measure zero sessions ("no data supports this");
  - checks the fingerprint and title against existing insights;
  - returns `{ drafts: [{ draft_id, title, …, evidence: [{ claim, value, sessions, window }] }], refused: [{ title, reason }] }`, plus an instruction to show these to the person.
- **`save_insights { draft_ids }`** (not read-only, `user` role):
  - inserts each draft exactly as drafted, with its measurements as the first history points;
  - deletes the draft;
  - returns the new ids;
  - refuses unknown or expired ids and drafts from another website.
- **`add_insights`** is removed rather than kept as an alias. A silent "draft and save" would defeat the point. A call to it returns an error naming the two new tools.
- **Prompt and instructions:** `analyse_website` step 5 becomes "draft_insights, show the person each draft with its measured evidence, then save_insights with the ones they approve". The server `INSTRUCTIONS` say the same.

### D5. Measurements and history
- **Table:** `insight_measurements (id, insight_id, item INTEGER, value REAL, sessions INTEGER, window_from, window_to, measured_at, kind TEXT)`.
  - `item` is the index of the evidence item.
  - `kind` is `drafted`, `refresh`, `before` or `after`.
- **`insights::refresh(pool, insight)`** re-measures each measured item over `[now - days, now)`, where `days` is the length of the first window.
- **Daily refresh:** the hourly loop calls `refresh_due`, which refreshes insights that aren't dismissed and have no `refresh` measurement in the last 24 hours, at most 200 per run.
- **On demand:** `refresh_insight` (MCP, `user` role, not read-only, since it writes history), `POST /insights/{id}/refresh`, and a dashboard button.
- **Around a release:** `evaluate::run` also measures every measured item over the same before and after windows as the KPI. It stores `before` and `after` rows and an item verdict (improved, worsened, no change, not enough data) using `welch`, with `goal` as the direction. This settles together with the KPI evaluation.
  - These are stored in `evaluation_items (insight_id, item, verdict, summary, settled_at)`, so the KPI evaluation row's shape stays the same.
  - A released insight with no KPI linked still gets item verdicts, so "Not measured" now only applies when an insight has neither a KPI nor any measured evidence.
- **Pruning:** history rows are deleted with their insight, not by event retention. They are aggregates, like evaluations.

### D6. Provenance
- **New `insights` columns:**
  - `created_by TEXT`: the actor string, for example `deyan@example.com via Claude Code (signed in 2026-10-28)`, `Trace's built-in checks` or `Trace's Claude analyst`;
  - `created_via TEXT`: `agent`, `heuristics`, `claude` or `demo`, which is what the public view shows (`source` already holds this; `created_via` is kept as an alias for clarity in API output);
  - `measured_from`, `measured_to`.
- **Actor strings:** MCP's `Context.actor` currently reads `mcp: <email> via token "<name>"`. It becomes `<email> via <token name>`. That is the same information, worded for display.
- **Prompt:** `prompt.rs` adds "Found by {created_by} on {date}, from behaviour between {from} and {to}." The public view's `insight_json` adds `created_via` and `created_at` but not `created_by`.

### D7. Built-in checks and the Claude analyst
- **Heuristics:** each finding in `heuristics.rs` already knows its element and page, and maps directly onto measurements:
  - rage clicks: `session_rate` of `rage_click` with path and selector, `goal: decrease`;
  - dead clicks: likewise with `dead_click`;
  - errors: `session_rate` of `error` with path and name;
  - forms: `conversion` from `form_start` to `submit` with path and selector, `goal: increase`.

  The existing sentences become the claims.
- **Claude analyst:** its output schema (`analyst.rs`) gains the evidence object. Items without a valid metric become notes, and a run whose insights have no metric at all is treated like a refused draft.

## Risks / Trade-offs

- **More expressive queries could narrow down to one visit.** Mitigation: aggregates only, merging of small rows into "other", no breakdown by session, visitor or time of day finer than a day, and scrubbed text at ingest.
- **Metric specs written by agents might not measure what the claim says.** Mitigation: the person sees the claim with Trace's number before saving, and the number is always Trace's. A claim that says "most" next to "3%" is visible.
- **The daily refresh adds load.** It is bounded: at most 200 insights per run, each with a handful of SQL statements against indexed columns.
- **Removing `add_insights` breaks agents that call it.** It is only a few days old, the error message says what to use instead, and the README and prompt change together.
- **Existing free-text evidence becomes "not measured".** It's honest, but older insights look weaker. They can be re-drafted by an agent if wanted.

## Migration Plan

- `db::migrate` adds the new `insights` columns, the two tables, and the `events (project_id, type, ts)` index on both databases.
- Old evidence arrays of strings load as notes, with no rewrite.
- Deploy order doesn't matter: the dashboard reads both evidence shapes.
- Rollback: the old code reads the new evidence JSON as `json_list` and gets an empty list (unknown shape), which loses the display but not the data. The columns can stay.

## Decisions taken while planning

- **Read-only connections can draft.** Drafting writes only an expiring draft that nobody else sees, so a read-only agent can still analyse and show the person what it found. It can never save. Read-only means "it can look and suggest, but never change anything".
- **Small rows: 3 sessions inside the account, 5 in the public view.**
  - **Why any limit inside the account:** the fixed digest shows rows of any size, but queries can be combined (page × device × day), and so could narrow down to a single visit. A minimum of 3 means every row describes at least three different visits.
  - **Why not higher:** a higher limit would hide too much on small websites, which still need their rarer pages. Rows under the limit aren't dropped: they're added into "other", so totals stay right.
  - **Why 5 in public:** the public view keeps its existing, stricter 5, because strangers see it.
