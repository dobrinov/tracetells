# Proposal

This is a change to onboarding and the MCP server.

## Why

Everything now happens in the person's agent, but onboarding started with "add the tag", only Claude Code had guided workflows, and an analysis took 7 to 10 tool calls that smaller local models struggle to chain.

## What Changes

- **Onboarding starts with connecting the agent** (now required): the agent then adds the tag, writes trace.json and analyses.
- **MCP prompts for every client:** `setup_website`, `fix_insight` and `manage_trace`, served from the Claude Code plugin's own SKILL.md files (one source), beside `analyse_website`. Each takes `request` and `site`.
- **`get_brief`:** everything an analysis starts from in one call (goals, the worst funnel step, areas that hurt, errors, rage and dead clicks, abandoned forms, pages people leave, bounce and engagement, existing insights with dismissal reasons). The analyse prompt and skill start from it.
- **Kept:** the REST write endpoints the dashboard no longer calls stay as the scripting API for tokens and CI.

## Capabilities

### Modified Capabilities
- `mcp`: prompts for every workflow, and a one-call brief.

## Privacy impact

None: the brief is the same aggregates as the other tools.

## Site impact

None.
