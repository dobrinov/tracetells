# Proposal

This is a feature.

## Why

The dashboard opened on Insights (or Get started), and switching websites kept the page of the previous one. The owner wants a home per website showing what is most relevant at that moment.

## What Changes

- **Home** (`#/home`, `GET /api/sites/{site}/today`): a ranked list of what needs attention now, each with a link to act on it: unfinished setup (first until data arrives), a verdict settled in the last week, analysis being due, insights to review or in progress, releases being measured, KPIs short of target and getting worse, the funnel step losing the most sessions, errors this week, declared events not arriving. Plus sessions this week against the week before.
- Every website opens on Home, including after switching websites, which no longer keeps the previous website's page.
- `get_overview` over MCP includes the same list as `today`.

## Capabilities

### New Capabilities
- `website-home`: what needs attention on a website now.

### Modified Capabilities
None.

## Privacy impact

None: the list is built from aggregates Trace already shows.

## Site impact

None: the collector is unchanged.
