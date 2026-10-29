# Proposal

This is a feature.

## Why

The owner wants the numbers every product team starts from: how many people visit, how many bounce on the marketing pages, how many do something that makes them engaged (a sign-up or a first real use), DAU/MAU and retention. Trace measured sessions, KPIs and funnels, but not its audience.

## What Changes

- **`audience` in `trace.json`:** `marketing` (path patterns of the marketing pages) and `engaged` (event matchers for the actions that make a visitor engaged), validated like KPIs and stored with the applied config. `trace check` shows them.
- **Measurement (`trace_core::audience`):** visitors and sessions per day; bounce on the marketing pages (landed and left without an interaction or a second page) by landing page and device; the share of visitors who did an engaging action, per action; DAU (7-day average), WAU, MAU and stickiness, and weekly retention cohorts for people the app signs in with `trace.identify()`. Each with the window before.
- **Dashboard:** an Audience page, and the last 7 days of it on Home.
- **Agents:** `GET /api/sites/{site}/audience` and the `get_audience` MCP tool; the analysis prompt and the plugin's analyse skill read it.
- **Demo:** the Stride shop declares its audience, and some simulated visits are signed-in members, so the demo shows DAU and retention.

## Capabilities

### New Capabilities
- `audience`: visitors, bounce, engagement, active people and retention.

### Modified Capabilities
None.

## Privacy impact

None new. Everything is a count. Anonymous visitors still can't be followed across days: DAU, MAU and retention use only the ids a website passes to `trace.identify()`, which already existed, and the docs ask for a keyed hash rather than anything that names a person. Nothing is added to public views.

## Site impact

None: the collector is unchanged.
