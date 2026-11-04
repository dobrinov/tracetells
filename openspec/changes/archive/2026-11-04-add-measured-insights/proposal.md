# Proposal

This is a feature.

## Why

Agents are Trace's analysts, but today they work blind and write in free text:
- `get_behaviour_summary` is one fixed digest, so an agent can't ask a follow-up question ("does this happen on mobile only?", "since when?").
- `add_insights` saves whatever the agent types as evidence. Trace can't check it, and it never changes again.

As a result, an insight can claim numbers that aren't in the data, nobody can tell who created it or from which data, and after a change ships there's no way to see whether the problem it describes went away. The only signal is the KPI, and that is often too broad to move on its own.

People should be able to ask Trace questions about how their customers behave. An agent should be able to turn the answers into insights whose evidence is measured by Trace, shown to the person before anything is saved, and re-measured over time. The history around a release then shows whether the change helped or hurt.

## What Changes

- **Asking questions.** A new MCP tool, `query_behaviour`, also available at `GET`/`POST /api/sites/{site}/query`, answers aggregated questions.
  - It uses the KPI matcher language (pageview, click, rage click, dead click, error, custom event, form start and submit, with path, selector or name) and the measures `sessions`, `events`, `session_rate`, `per_session` and `conversion` (from → to).
  - Results can be broken down by `path`, `selector`, `device` or `day`, over a window of days, optionally compared with the previous window.
  - It returns aggregates only, never sessions or people, and runs as a few SQL queries.
- **Evidence as measurements.** Each evidence item of an insight is a claim plus a metric spec that Trace measures itself (the same shape `query_behaviour` takes).
  - Trace stores the value, the number of sessions it covers and the window. The claim text shows Trace's number, not the agent's.
  - An insight needs at least one measured item. Written notes are allowed, labelled "not measured", but are never the only evidence.
- **Draft, show, then save.**
  - `draft_insights` validates proposed insights and measures their evidence. It returns drafts with real numbers and draft ids; drafts are not insights, aren't listed, and expire after an hour.
  - The agent must show them to the person, and `save_insights` saves only the drafts named.
  - The `analyse_website` prompt and the server instructions require this order.
  - **BREAKING (MCP):** `add_insights` is removed. Clients are told to use `draft_insights` and then `save_insights`.
- **Provenance.** Insights record who created them and through what (for example "deyan@… via Claude Code (signed in 2026-10-28)", or "Trace's built-in checks"), when, and the window their evidence was measured over. The dashboard, the API, MCP and the task prompt show this.
- **Refresh and history.**
  - Trace re-measures every open or released insight's evidence once a day, and on demand: `refresh_insight` in MCP, `POST /api/sites/{site}/insights/{id}/refresh`, and a button on the insight.
  - Every measurement is kept, so each evidence item has a history.
  - For a released insight, the evidence is also judged over the same fixed windows as its KPI (14 days before and after the release), next to the KPI verdict. This shows whether the specific problem went away.
- **Built-in checks** (heuristics) emit metric specs for their evidence, so their insights are measured and refreshed the same way. The server's optional Claude analyst does too.
- **Connect screen copy.** The read-only box says that an agent with read-only access can ask questions but can't save insights.
- **Public view.** Shows insights' measured evidence, with rows from fewer than 5 sessions left out as elsewhere.

## Capabilities

### New Capabilities
- `behaviour-queries`: asking aggregated questions about behaviour over MCP and the API.
- `measured-insights`: insights whose evidence is measured by Trace, drafted and shown before saving, with provenance.
- `insight-history`: re-measuring insights' evidence over time and judging it around a release.

### Modified Capabilities
None: the existing changes aren't archived, so there are no main specs to modify. This change supersedes the `add_insights` requirements in `add-agent-analysis`.

## Privacy impact

- **Collected:** nothing new. The collector is unchanged.
- **Exposed:**
  - `query_behaviour` gives agents more ways to slice the same aggregates. It never returns session ids, visitor ids, user ids, user agents or single events.
  - Every result row carries its session count. Breakdowns by `selector` and `path` only show values already scrubbed at ingest, and rows from fewer than 3 sessions are merged into "other", so a breakdown can't single out one visit's rare page.
  - For the public view, the threshold is 5 sessions, as today.
- **Stored:**
  - drafts, for up to an hour;
  - per insight, the metric specs and their measurements (aggregate numbers with a window and session counts);
  - who created the insight, as the person's account email plus the client or token name. It is shown inside the account only, never in the public view.
- Text in metric specs (paths, selectors, names) is validated and treated as untrusted, as with other visitor-derived text.

## Site impact

None: the collector is unchanged.

## Impact

- **`crates/trace-core`:**
  - new `query.rs` (metric specs, SQL-backed measuring, breakdowns);
  - `insights.rs` (evidence items, provenance, drafts, measurements, refresh);
  - `model.rs`, `evaluate.rs` (evidence verdicts beside the KPI verdict), `prompt.rs`;
  - both schemas: new `insight_drafts` and `insight_measurements` tables, and new `insights` columns.
- **`crates/trace-agent`:** `heuristics.rs` and `analyst.rs` emit metric specs.
- **`crates/trace-server`:**
  - `mcp.rs`: new tools, `add_insights` removed, prompt and instructions;
  - `api.rs`: query, refresh and the evidence history in insight responses;
  - `public.rs`, `main.rs` (daily refresh in the background loop), and the dashboard (evidence with history, provenance, Refresh).
- **Docs:** the README's MCP and insights sections.
- **Existing insights:** their free-text evidence becomes "not measured" notes. They keep working, but have no history until they are refreshed with measurable evidence (see design).
