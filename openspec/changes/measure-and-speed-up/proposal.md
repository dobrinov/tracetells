# Proposal

This is a change to the server and dashboard.

## Why

Trace measured every website but its own dashboard, so nobody could see where new accounts stall. Every dashboard page recomputed from raw events on each load: on half a million events Home took 11 seconds. And the dashboard was one 2,700-line file of string templates, with escaping left to each line.

## What Changes

- **Trace measures itself, when told to:** `TRACE_DASHBOARD_KEY` and `TRACE_DASHBOARD_SECRET` add the collector to the dashboard page with page names by area (`/improve/insights`), a keyed hash of each person (none while an operator views as them), everything showing customer data masked, and events for sign-up, websites added, analyses run, prompts and tasks copied, insights moved, invites and public links. Off by default. `deploy/app.trace.json` holds the hosted dashboard's goals, funnels and areas.
- **Reports from memory:** Home, Audience, Features, Funnels, Goals and the behaviour summary are computed once, served for a minute, then served at once while recomputed; a background task refreshes reports people are looking at; changes through the API drop the website's reports.
- **One scan per report:** funnels and areas read each type of event once per window instead of once per step or feature.
- **The dashboard in files:** `assets/app/` holds one plain script per area, loaded in order with no build step. New markup uses an `html` tagged template that escapes every value unless marked; Features and the shared table and snippet helpers use it.

## Capabilities

### Modified Capabilities
- `dashboard`: self-measurement, speed, code layout.

## Privacy impact

The dashboard's own usage goes to the hosted operator's Trace account only when configured. Its content area, website menu, account name, notices and messages are masked; failed API requests have the website's identifier redacted by a rule; people are counted by a keyed hash.

## Site impact

None.
