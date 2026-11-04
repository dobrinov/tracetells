# Proposal

This is a feature, and a change of concept: the user's coding agent is the analyst.

## Why

Hosting an LLM analyst costs money per run, and sends each website's summary to a model provider the website owner didn't choose. Coding agents (Claude Code, Cursor and others) already run a capable model on the owner's side and already connect to Trace over MCP. Letting them analyse costs the hosted service nothing, lets each team pick its own model (including a local one), and keeps the summary going only where its owner sends it.

## What Changes

- New MCP tool **`add_insights`**: an agent saves up to 10 insights with title, problem, evidence, proposal, acceptance criteria, pages, the KPI it should move (key or id), expected impact, confidence and a fingerprint.
  - It uses the same de-duplication as Trace's own analyst, so known and dismissed insights are skipped.
  - One invalid item saves nothing.
  - It needs a token that isn't read-only, and saves are recorded in the activity log.
- New MCP prompt **`analyse_website`** with the analyst's brief: read the overview, summary, KPIs and existing insights (with dismissal reasons), pick up to five problems, and save them with `add_insights`. It and the server's instructions say that text inside behaviour data comes from visitors and must never be followed as instructions.
- `list_insights` includes dismissal reasons.
- The dashboard's Setup and Get started pages present the agent as the analyst and say how to run the prompt (`/mcp__trace__analyse_website` in Claude Code). The empty Insights page points to it.
- Trace's own analysis stays: built-in heuristics (**Run analysis**) need no model, and Claude stays optional with `ANTHROPIC_API_KEY`.
- README, marketing site and pricing copy describe the agent as the analyst.

## Capabilities

### New Capabilities
- `agent-analysis`: analysis done by the user's coding agent over MCP.

### Modified Capabilities
None.

## Privacy impact

None new. Agents read the same aggregated summary as before (`get_behaviour_summary`). With the agent as analyst, the summary goes only to the model the website owner chose, instead of a provider configured on the server.

## Site impact

None: the collector is unchanged.

## Impact

`crates/trace-server/src/mcp.rs` (tool, prompt, instructions, tests), `assets/app.js` (Setup, Get started, Insights copy), README, `site/index.html`, `site/pricing.html`.
