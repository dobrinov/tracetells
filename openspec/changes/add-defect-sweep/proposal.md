# Proposal

This is a feature, plus a bug fix in when the first analysis is worth running.

## Why

A small website gets nothing from Trace for a long time. Analysis waits for 100 new sessions, and the lower bar of 30 only applies a week after an earlier analysis, so a site that has never been analysed always needs 100. wunderkind.bg had 27 sessions after two weeks and `/trace:analyse` stopped. Yet its data already showed things that were plainly broken: about 190 failed requests to an analytics endpoint, a `POST /sign-in → 422`, a funnel whose second step (`/sign-up`) no session reached, and declared events (`signup`, `goal_created`) that never arrived. None of those needs a sample size. One occurrence, confirmed in the code, is enough.

Lowering the pattern threshold instead would make insights out of noise. It would also show behaviour drawn from one or two sessions, which comes close to describing one person. Defects describe the website, not its visitors.

## What Changes

- **A defect sweep.** Trace lists defect candidates from data it already has, from the first session on:
  - failing requests (network errors and 4xx/5xx), JS exceptions and console errors;
  - missing files (404s);
  - funnel steps that no session reaches although the step before has sessions;
  - custom events declared in trace.json that have never arrived;
  - dead clicks on elements whose selectors look interactive (buttons, links, form fields, elements with a role).

  Each candidate lists the pages it was seen on, its count and how many sessions it affected. A new MCP tool, `get_defects`, returns them.
- **Defect insights.** An insight has a kind, either `pattern` (as today) or `defect`. A defect may be drafted when its measurement is zero (an event that never arrives, a step nobody reaches) and needs no KPI. Its acceptance is "it stops happening", or "it starts arriving". After release, Trace judges it by whether the failure recurs, not by a significance test.
- **`/trace:analyse` and the `analyse_website` prompt.** When there isn't enough behaviour for patterns, they run the defect sweep instead of stopping. The agent confirms each candidate in the codebase before drafting it, and shows the person what it found in the code. Above the threshold, the analysis covers both.
- **Readiness (bug fix).** Before a website's first analysis, the week that unlocks the 30-session threshold counts from its first session. `get_analysis_status` also says when a defect sweep is worth running below the threshold: there are defect candidates that no insight covers yet.
- **Dashboard.** Insights show a "Defect" tag, and the analysis panel offers the sweep when patterns aren't ready yet.

## Capabilities

### New Capabilities
- `defect-sweep`: finding defect candidates in collected data, drafting them as defect insights, and judging them after release.

### Modified Capabilities
None. No main specs exist yet (`openspec/specs/` is empty). The readiness change adds requirements to the in-flight `analysis-readiness` capability from `add-analysis-readiness`, as `guide-analysis` did.

## Privacy impact

None in collection: no new data is collected or stored. The candidates are aggregates (counts and session counts per page, selector or message) of data that `get_behaviour_summary` already exposes. Defect insights describe the website's failures, not anyone's behaviour. The sweep is meant for low traffic, where pattern analysis would expose behaviour from very few sessions, so it lowers the risk.

## Site impact

None: the collector is unchanged.

## Impact

- `trace-core`: a new `defects` module (candidates from errors, funnels, the event catalogue and dead clicks), `readiness.rs` (first-session week, sweep status), `insights.rs` and `model.rs` (insight kind, defect drafting rules), `evaluate.rs` (defect verdicts), and an additive `insights.kind` column in `schema.sql` and `structure.sql`.
- `trace-server`: `mcp.rs` (`get_defects`, `draft_insights` accepting `kind`, the `analyse_website` prompt, `get_analysis_status`), the HTTP API for the dashboard, and `assets/app/` (Defect tag, analysis panel copy).
- `plugins/trace/skills/analyse/SKILL.md` and `fix` (a defect is done when it stops happening).
- README and tracetells.com docs on what Trace does for small sites.
- Follow-up, not in this change: reading a wider window (30 or 90 days) automatically when 14 days is thin.
