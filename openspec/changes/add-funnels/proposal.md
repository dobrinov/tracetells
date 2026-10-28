# Proposal

This is a feature.

## Why

Conversion KPIs measure one step at a time, so nobody sees the whole path people take and where most of them stop. The owner wants funnels recognised and shown, and analysis aimed at what would raise conversion.

## What Changes

- **Funnels in `trace.json`:** `"funnels": { key: { name, description, steps: [matcher, ...] } }`, 2 to 8 steps, validated like KPIs and stored with the applied config. `trace check` lists them.
- **Recognised funnels:** conversion KPIs whose goal is the next one's start are chained into funnels of three or more steps, longest first, skipping any a declared funnel covers.
- **Measurement:** per step, sessions that reached it after the step before in the same session; the share of the step before and of the first; sessions lost; the previous window; conversion by device; and the biggest drop.
- **Dashboard:** a Funnels section on the KPIs page, also on public views (funnels from fewer than 5 sessions are left out there).
- **Analysis:** `get_funnels` over MCP and `GET /api/sites/{site}/funnels`; funnels in the behaviour digest; the analyse_website prompt, the plugin's analyse skill and Trace's own analyst start from the biggest drop; the built-in checks add an insight for a step that loses many sessions, with its conversion as evidence.

## Capabilities

### New Capabilities
- `funnels`: declaring, recognising, measuring and showing funnels, and analysing them for conversion.

### Modified Capabilities
None.

## Privacy impact

None: funnels are counts of sessions per step, built from events already collected. Public views leave out funnels and device rows from fewer than 5 sessions, like every other public row.

## Site impact

None: the collector is unchanged.
