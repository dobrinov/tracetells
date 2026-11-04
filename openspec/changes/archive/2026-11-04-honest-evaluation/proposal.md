# Proposal

This is a correctness fix to how releases are judged.

## Why

The verdict on a released insight could mislead in three ways:
- The after-window grew every hour, up to 14 days, and was re-tested each time. Checking a growing window over and over finds a "significant" swing far more often than 5% of the time, and verdicts could flip from one day to the next.
- The release was always judged by the KPI's current definition, so editing `trace.json` after a release silently changed what was measured.
- The split point was when someone clicked "released", not when the change went live.

## What Changes

- **Fixed horizon:** a release is compared over exactly 14 days before and 14 after. Until those 14 days pass, the verdict is `measuring` ("day 3 of 14", with the date the verdict comes) and has no numbers. After that, the evaluation is settled (`settled_at`) and no longer recomputed.
- **KPI as shipped:** releasing stores the linked KPI's definition and direction (`insights.released_definition`, `released_direction`), and the release is judged by them. If the KPI has changed since, the summary says so. Linking another KPI, or correcting the release time, starts the measurement over.
- **Real release times:**
  - Releasing accepts `released_at` (ISO 8601 or epoch ms) in `POST /insights/{id}/status`, MCP `update_insight_status`, `trace status … --at` and the dashboard's release prompt.
  - It is refused in the future, or before the insight was found.
  - `POST /api/sites/{site}/releases` lets a deploy mark several insights released at its time, using `Trace-Insight: N` commit trailers that the task prompt now asks for.
- **Overlap:** a verdict names other releases on the same KPI within 14 days.

## Capabilities

### New Capabilities
- `release-evaluation`: how Trace decides whether a released insight moved its KPI.

### Modified Capabilities
None.

## Privacy impact

None.

## Site impact

None: the collector is unchanged.

## Impact

- `trace-core`:
  - `evaluate.rs` (rewritten loop), `insights.rs` (`set_status` with a release time, `snapshot_release`, `released_kpi`);
  - `model.rs` (`Verdict::Measuring`, `Evaluation::settled_at`), `util.rs` (`parse_time`, `date`);
  - both schemas and the migration, `prompt.rs`.
- `trace-server`: `api.rs` (`released_at`, `/releases`), `auth_api.rs`, `mcp.rs`, `main.rs` (`--at`), `demo.rs` (released 14 days ago so it shows a verdict), and the dashboard.
- The README gains a "Did it help?" section with a CI snippet.
- Existing evaluations are recomputed under the new rules. Releases from before this change have no snapshot and use the current definition.
