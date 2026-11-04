# Proposal

This is a change to the dashboard, following the design review on the canvas.

## Why

The dashboard grew one page per feature: eleven tabs, one page doing three jobs, setup steps that counted settings instead of results, a header that said "Not collecting yet" beside hundreds of sessions, and chips that coloured a 0% change red. Replay customers had no page that spoke to them.

## What Changes

- **Four areas:** Home, Understand (Audience, Features, Funnels, Pages, Errors), Improve (Goals, Insights, Releases) and Setup (Get started, Install and tracking, Privacy, Team, Plan), with the pages of an area as tabs under the header. Addresses carry the area (`#/understand/funnels`); old links redirect.
- **Split pages:** KPIs became Goals; funnels got their own page; tracking health moved to Setup › Install and tracking; errors left Pages; released insights became Releases.
- **Header status:** live, quiet, waiting for the first visit or stopped, with the time of the last event. The Run analysis button shows only when an analysis is due.
- **Chips and samples:** a change says better or worse by the goal; counts without a goal are neutral; "new" when there's nothing before. A goal under 10 sessions shows no value, under 30 shows "early".
- **Features:** where to look first, the product map, use against problems, and a page per area with its features, problems ("Make it a task"), funnels and insights. "Analyse this" copies `/trace:analyse <area>`; the analyse prompt and plugin skill take an area.
- **Onboarding:** three outcomes (first visit, goals, first insight); agent, privacy and team are optional.
- **tracetells.com:** a "Switching from replay" page for teams on FullStory, Hotjar and similar.

## Capabilities

### Modified Capabilities
- `dashboard`: navigation, page split, status, chips, onboarding and Features.

## Privacy impact

None: the same aggregates, arranged differently.

## Site impact

None: the collector is unchanged.
