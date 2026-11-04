# Tasks

## 1. Measuring in SQL

- [x] 1.1 `query.rs`: `Metric` (measure, matchers in trace.json shorthand or full form), parsing and validation (event types, text length and characters, a 1–90 day window capped by retention)
- [x] 1.2 `query::measure` over `[from, to)`:
  - push the matchers into SQL (exact, and prefix `LIKE` with `%` and `_` escaped);
  - `sessions`, `events`, `session_rate`, `per_session`, and `conversion` with ordering;
  - optional per-session samples
- [x] 1.3 Breakdowns by `path`, `selector`, `device` and `day`: at most 50 rows, largest first, values under 3 sessions merged into "other"; `compare` adds the previous window
- [x] 1.4 Add the `events (project_id, type, ts)` index in `db::migrate` (both databases)
- [x] 1.5 Tests on SQLite and Postgres:
  - each measure against hand-built events;
  - prefix and escaping;
  - conversion order;
  - the "other" merge;
  - `compare`;
  - `session_rate` and `conversion` agree with `kpi::measure` on the same data

## 2. Asking questions

- [x] 2.1 MCP `query_behaviour`, open to read-only tokens and rate-limited; the result says what was measured, over which dates, with session counts
- [x] 2.2 `GET`/`POST /api/sites/{site}/query` with the same input and output; add it to the site route list and the isolation test
- [x] 2.3 Tests: refusing a breakdown by session or visitor; nothing in a result names a session, visitor or user agent; read-only tokens can query

## 3. Evidence, drafts and saving

- [x] 3.1 `model::Evidence` (`Measured { claim, metric, goal }` or `Note`); read old string arrays as notes; `insights` columns `created_by`, `measured_from`, `measured_to`; the `insight_drafts` and `insight_measurements` tables (both schemas and the migration)
- [x] 3.2 `draft_insights`:
  - validate;
  - measure every item;
  - refuse insights with no measured item, or whose metrics all cover zero sessions;
  - check fingerprints and titles;
  - store drafts for an hour and return them with Trace's numbers and an instruction to show them
- [x] 3.3 `save_insights { draft_ids }`:
  - needs write access;
  - saves drafts exactly as drafted, with the first measurements as history;
  - refuses unknown, expired or other-website ids
- [x] 3.4 Remove `add_insights` (calls get an error naming the two new tools); update `analyse_website` and the server `INSTRUCTIONS` to draft, show, then save
- [x] 3.5 Provenance:
  - the actor string reads as `<email> via <token name>`;
  - built-in checks and the Claude analyst record their own names;
  - show it in the API, MCP (`list_insights`, `get_insight`) and the task prompt
- [x] 3.6 Tests:
  - draft then save two of three;
  - an expired draft;
  - an all-notes insight refused;
  - a duplicate refused;
  - drafts not listed as insights;
  - a read-only token can draft but not save;
  - the prompt shows who, when and the window

## 4. History and releases

- [x] 4.1 `insights::refresh`: re-measure measured items over the first window's length ending now, and store `refresh` rows; `refresh_due` in the hourly loop (insights that aren't dismissed and weren't refreshed in 24 hours, at most 200 per run)
- [x] 4.2 `refresh_insight` (MCP), `POST /api/sites/{site}/insights/{id}/refresh`; insight responses include each item's history
- [x] 4.3 `evaluate::run` measures items over the release's before and after windows, and stores `before`/`after` rows and `evaluation_items` verdicts (Welch, with the item's goal), settling with the KPI evaluation; released insights without a KPI still get item verdicts
- [x] 4.4a Heuristics emit measured evidence (rage, dead clicks, errors: of sessions viewing the page; forms: start → submit)
- [x] 4.4b The Claude analyst's output schema gains the evidence object (its evidence is notes until then)
- [x] 4.5 Tests:
  - refreshing adds history and keeps the text;
  - daily refresh selection;
  - item verdicts before and after a release, including "better locally, worse overall";
  - heuristics evidence is measurable and matches the digest

## 5. Dashboard, public view and copy

- [x] 5.1 Insight card:
  - measured evidence with Trace's number, sessions and dates;
  - notes marked "not measured";
  - a small history per item;
  - "Found by … on …";
  - a Refresh button for users
- [x] 5.2 Released insights: item verdicts next to the KPI verdict
- [x] 5.3 Public view: measured evidence and history with the 5-session suppression, and `created_via` and the date but not the person; extend the leak test
- [x] 5.4 Connect screen: replace the "Read only" checkbox with two choices, Full access (the default, described) and Read only (described); the token name says which
- [x] 5.5 Check the dashboard in a browser: measured evidence, refresh and its history, a released insight's KPI and evidence verdicts (draft and save over MCP are covered by tests, not yet by a real Claude Code session)
- [x] 5.6 README: `query_behaviour` with examples, measured evidence, draft-then-save, refresh and history, the removal of `add_insights`
