# Proposal

This is a feature.

## Why

With the coding agent as the analyst, nothing on the server decides when to analyse, so the owner has to remember. Trace knows how much new behaviour has arrived, so it should say when an analysis is worth running: in the dashboard, to the agent, and in the owner's editor when they start work.

## What Changes

- Each website records when it was last analysed: an agent calling `add_insights` (an empty list means "looked, found nothing") or Trace's own analysis.
- Trace counts the sessions since then. It's worth analysing at 100 new sessions, or 30 once a week has passed.
- That status, with a one-line message, appears:
  - on the Insights page;
  - in MCP `get_overview` and a new `get_analysis_status` tool;
  - at `GET /api/sites/{site}/analysis`, which also offers `?format=text` (one line, or nothing) and `?format=claude-hook` (`{"systemMessage": …}`, or nothing).
- The Setup page has a ready Claude Code `SessionStart` hook built on it.
- Sign-up can link terms and a privacy policy (`TRACE_TERMS_URL`, `TRACE_PRIVACY_URL`). The task prompt says who found an insight, an agent or Trace, from aggregated data.
- tracetells.com gets `privacy.html` and `terms.html`. The consent bar's "How it works" link and every footer point to them.

## Capabilities

### New Capabilities
- `analysis-readiness`: telling the owner and their agent when there is enough new behaviour to analyse.

### Modified Capabilities
None.

## Privacy impact

None: one timestamp per website, and counts of sessions that are already stored.

## Site impact

None: the collector is unchanged.

## Impact

- `crates/trace-core`: `readiness.rs`, and `projects.analysed_at` in both schemas.
- `crates/trace-server`: `mcp.rs`, `api.rs`, `analysis.rs`, `auth_api.rs`, `main.rs`, and the dashboard.
- Docs: README and `deploy/README.md`.
- `site/`: the privacy and terms pages and the footers.
