# Proposal

This is a change to the product's shape.

## Why

The expectation is that a person analyses and manages Trace through their own coding agent, over MCP, with the model they choose (local or hosted). The server could analyse on its own (Claude with an API key, or built-in heuristics), and the dashboard could change insights, privacy rules, the public link and websites. That made two ways to do everything, and a server that could send data to a model the customer didn't choose.

## What Changes

- **No server-side analysis:** the trace-agent crate (Claude client, analyst, heuristics), `POST /analyze`, `GET /runs`, the `run_analysis` MCP tool, `trace analyze`, the `ANTHROPIC_API_KEY` / `TRACE_MODEL` / `TRACE_EFFORT` settings and the "Claude analyses per month" plan limit are gone. The `analysis_runs` table and `plans.monthly_ai_runs` / `usage.ai_runs` columns stay (migrations are additive) but are no longer read or written.
- **The dashboard shows; the agent does:** insights, privacy rules, the privacy review, the public link and adding websites are changed through the agent. Where there's something to do, the dashboard shows the exact command with a Copy button; Setup › Install and tracking lists them all. The header's "Analyse · N new" copies `/trace:analyse`.
- **`/trace:manage`:** a new plugin command for picking, dismissing, releasing and reopening insights, privacy rules and the review, the public link, adding a website, applying trace.json and checking that data arrives.
- **Stays on the website:** signing in, two-step verification, passwords, tokens, accounts, the team, approving an agent, deleting a website.

## Capabilities

### Modified Capabilities
- `dashboard`: presentation, with agent instructions instead of actions.
- `analysis`: only in the person's agent.

## Privacy impact

Positive: the server never sends a website's data to a model; only the person's own agent reads the aggregated summaries.

## Site impact

None.
