# Proposal

This is a feature.

## Why

Larger products have sub-products, and those have features. Some get used and others don't, and some are used a lot but hurt. The owner wants to see engagement and UX problems per area, so attention goes where it helps the most people.

## What Changes

- **`areas` in `trace.json`:** areas, each with `name`, `description`, `match` (event matchers) and/or `features` (the same, one level down). Up to 16 areas with 24 features each. Validated like KPIs; `trace check` lists them.
- **Measurement (`trace_core::areas`):** per area and feature over the window:
  - sessions, and reach (visitor-days), compared with the window before;
  - adoption among signed-in people, and repeat use (two or more days);
  - sessions with a rage click, dead click or error on its own pages, and the most common one;
  - a status: healthy, painful, rare, rare_painful or too_little.
- **Suggested areas:** without declared areas, Trace builds them from the first URL segment, with the second as features.
- **Dashboard:** a Features page (groups by status, then a table of areas and features). Home raises the busiest painful area.
- **Agents:** `GET /api/sites/{site}/areas`, the `get_areas` MCP tool. The analysis prompt and plugin skill point at painful areas.
- **Declared for:** the Stride demo shop and wunderkind.bg.

## Capabilities

### New Capabilities
- `areas`: product areas and features, their use and their problems.

### Modified Capabilities
None.

## Privacy impact

None: counts of sessions, visitor-days and signed-in ids already collected. Nothing is added to public views.

## Site impact

None: the collector is unchanged.
