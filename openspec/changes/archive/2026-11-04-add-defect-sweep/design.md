# Design

## Context

- **Readiness** (`trace-core/src/readiness.rs`): `decide` uses 30 sessions only when `last` (the last analysis) is at least a week old. With no analysis, `a_week_on` is false, so a new website needs 100. `mark_analysed` sets `projects.analysed_at`. The MCP `save_insights` handler in `trace-server/src/mcp.rs` calls it on every save, including an empty one.
- **Drafting** (`mcp.rs`, `draft_insights`): `agent_insight` validates an item, `insights::duplicate` checks fingerprint and title, and `insights::measure_evidence` takes the first readings. A draft whose readings are all zero is refused ("no collected data matches its measurements"). `NewInsight` (`model.rs`) has no kind. `kpi_id` is already optional.
- **Evaluation** (`evaluate.rs`): `evaluate_items` judges each measured item 14 days before against 14 days after the release. Counts (`sessions`, `events`) end as `InsufficientData` ("Counts aren't compared for significance"). Rates go through a Welch test with `opts.min_sessions` per side. For a defect, the useful question is simply whether it happened again, and neither path answers it.
- **Sources for candidates**, all already computed:
  - errors with their kind, message, path, count and sessions (as in `get_behaviour_summary`; the `digest` module);
  - funnel steps with `sessions` per step (`funnel.rs`);
  - declared events and whether they arrived (`config::tracking_health`, which already reads the event catalogue against `events`);
  - dead clicks by path and selector.
- **Skill and prompt**: `plugins/trace/skills/analyse/SKILL.md` and the `analyse_website` prompt in `mcp.rs` are kept in step (a comment in the skill says so). Step 0 stops when the status isn't ready.

## Goals / Non-Goals

**Goals:**
- Candidates computed on demand from existing tables, on SQLite and Postgres, with no new collection.
- One insight model for both kinds, so statuses, dedupe, `/trace:fix` and the dashboard keep working.

**Non-Goals:**
- Automatic sweeps by Trace itself, without an agent. The agent's check in the code is what makes a single occurrence trustworthy.
- Grouping browser failures with backend error groups from `add-server-errors`. A candidate for a failing request can link to one later.
- Reading wider windows automatically when 14 days is thin (a follow-up).

## Decisions

### Candidates live in a new `trace-core/src/defects.rs`
`defects::candidates(pool, project_id, days) -> Vec<Candidate>` assembles candidates from the four sources above. A `Candidate` has `key`, `kind` (`failing_request`, `exception`, `missing_file`, `unreached_step`, `missing_event`, `dead_control`), `label`, `pages`, `count`, `sessions`, `covered_by` (insight id and status) and a suggested `metric` in `query_behaviour` form. The agent can copy that metric into its evidence unchanged. Error candidates match their message with the matcher's new `text` pattern (cut to a prefix past 150 characters). Keys are stable strings, built the way fingerprints are today (`error:POST stats.example.com/api/event`, `funnel:acquisition:/sign-up`, `event:signup`, `dead:/checkout:button.pay`). `covered_by` matches a key against insight fingerprints, so an agent that uses the key as its fingerprint covers the candidate.

*Alternative:* a section in `get_behaviour_summary`. Rejected: the summary is already large, and the sweep has to work with only this list in hand.

### "Looks interactive" is decided from the selector
The collector sends a selector, not styles. A dead click becomes a candidate when the last part of its selector is `button`, `a`, `input`, `select`, `textarea`, `summary` or `label`, has a `role` attribute, or has a class containing `btn`, `button`, `link` or `choice`. That list sits in one function with tests. Anything else (a `p` inside a question, a layout `div`) stays a behaviour signal for pattern analysis. False positives are acceptable, because the agent confirms each one in the code.

*Alternative:* collect "cursor: pointer" in the collector. Rejected: it reads layout on the hot path, which the site-impact rules forbid.

### An insight kind, stored in a column
Add `kind TEXT NOT NULL DEFAULT 'pattern'` to `insights`, in `schema.sql` and `structure.sql` (additive, through `db::migrate`). `NewInsight` and `Insight` get `kind`. `draft_insights` accepts `kind`. For `defect`, the all-zero refusal applies only to items whose goal is `decrease`: a zero with goal `increase` *is* the evidence. Drafts carry the kind through `create_draft`/`take_draft`.

*Alternative:* infer it from the fingerprint prefix. Rejected: the kind changes validation and judgement, so it should be explicit.

### Defect verdicts reuse `Verdict`
In `evaluate_items`, a defect's items skip the significance test. For goal `decrease`, the verdict is `Improved` when the after-window count is 0 and its pages had sessions, `NoChange` when the failure recurred, and `InsufficientData` when the pages had no sessions. For goal `increase`, the verdict is `Improved` once the after-window count is above 0. A defect item also settles early: a recurrence settles it as `NoChange` at once, and the first arrival of a missing event settles it as `Improved`. Summaries use defect words ("stopped", "still happening", "fixed", "still missing"). A defect has no KPI, and `evaluations.kpi_id` is NOT NULL (changing that isn't additive), so a defect gets no overall evaluation row: its item verdicts are its verdict, and `list_insights` and the dashboard show them as its impact.

*Alternative:* new `Fixed`/`StillHappening` variants. Rejected for now: every place that reads `Verdict` (dashboard, MCP, digest) would need them, and the summary already carries the wording. This is easy to revisit.

### A sweep is recorded separately from an analysis
Add `projects.swept_at` (additive). A candidate is uncovered when no insight fingerprint matches it *and* it occurred after `swept_at`, so candidates the agent looked at and didn't confirm stop prompting until they happen again. Candidates for something that never happens (an unreached step, a missing event) have no time of occurrence, so after a sweep they stay quiet until an insight or a later full analysis picks them up; `get_defects` still lists them. When `save_insights` is called and the status was `sweep` (not ready for patterns), it calls `readiness::mark_swept`. When ready, it calls `mark_analysed`, which also sets `swept_at`. `get_analysis_status` gains `next: "sweep"` and `defects` (the count of uncovered candidates). `decide` gets the first session's time, taken from the same `MIN(started_at)` that `tracking_health` already reads, and uses it in place of `last` before the first analysis.

### Skill and prompt
Step 0 becomes: if `next` is `sweep`, run the sweep and say patterns need more sessions. If `next` is `wait`, say so and stop, as today. If it's `analyse`, also call `get_defects` alongside the brief. The sweep steps are: call `get_defects`, and for each uncovered candidate find the code (the request URL, the element, the funnel link, the `trace.track` call). Draft only the confirmed ones with `kind: "defect"`, the candidate's key as fingerprint and its metric as evidence. In the presentation, show the code reference next to Trace's measured evidence. `/trace:fix` tells the person that a defect is judged by whether it happens again.

## Risks / Trade-offs

- [Candidates from one visitor's environment: an ad blocker, an extension, a flaky network] → the agent must find the cause in the codebase before drafting; the skill says to skip and explain otherwise. The `sessions` count is shown, so "1 session" is visible.
- [A noisy third-party script floods the list] → candidates are grouped by request target and message, not by page, and capped at 20, ranked by sessions and then by count.
- [`swept_at` hides a real defect the agent wrongly rejected] → it comes back as soon as it happens again after the sweep.
- [Verdict wording drifts from the enum names in the dashboard] → the dashboard shows the summary for defect items, not the enum name.
- [A defect on a page nobody visits after release never settles] → `InsufficientData` after the window, as the spec says. The insight can be re-released.

## Migration Plan

Two additive columns (`insights.kind`, `projects.swept_at`). Existing insights become `pattern`, and existing websites have no sweep. Rollback is a backup, as for every migration. The schema comparison test covers both files.
