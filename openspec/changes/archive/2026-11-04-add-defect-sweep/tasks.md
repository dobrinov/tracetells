# Tasks

## 1. Readiness for small websites

- [x] 1.1 `readiness.rs`: `decide` takes the first session's time and uses it in place of the last analysis before the first one; tests for "14 days, 30 sessions → ready" and "3 days, 30 sessions → needs 100" pass under `cargo test -p trace-core readiness`
- [x] 1.2 `projects.swept_at` in `schema.sql` and `structure.sql` with the migration, plus `mark_swept`; `mark_analysed` also sets it; the schema comparison test passes on SQLite and Postgres

## 2. Defect candidates

- [x] 2.0 Event matchers take an optional `text` pattern for error messages (model, trace.json/query parser, the kpi and areas scans); tests: `{"error": "*", "text": "POST stats.example.com/*"}` counts only those failures, and matchers without `text` count as before
- [x] 2.1 `trace-core/src/defects.rs`: `Candidate` and `candidates()` from errors (failing requests, exceptions, console errors, missing files), funnel steps unreached after a reached step, declared events that never arrived, and dead clicks on interactive-looking selectors; stable keys, suggested metric, capped at 20; unit tests cover each kind, including the four wunderkind.bg cases in the spec
- [x] 2.2 The "looks interactive" selector test as one function, with tests for `button.pay` (yes), `div.question-body > p` (no), `[role=button]` (yes) and `li.choice-button` (yes)
- [x] 2.3 `covered_by` from insight fingerprints, and "uncovered" honouring `swept_at`; test that a candidate seen before a sweep is hidden until it recurs
- [x] 2.4 `get_analysis_status` / `status`: `next: "sweep"` and a `defects` count when not ready and candidates are uncovered; the message names the count and `/trace:analyse`; the `?format=text` and `claude-hook` outputs print it; tests for the three readiness scenarios

## 3. Defect insights

- [x] 3.1 `insights.kind` column (both schemas, migration, default `pattern`); `kind` on `NewInsight`, `Insight`, drafts and the insights API and MCP output; existing insight tests still pass
- [x] 3.2 `draft_insights` accepts `kind`; a defect with a zero reading and goal `increase` is drafted, and a pattern with only zero readings is still refused; MCP tests for both
- [x] 3.3 `save_insights` calls `mark_swept` when the status was `sweep` and `mark_analysed` when ready; test that saving two defects at 27 sessions leaves the 27 new sessions in place
- [x] 3.4 `evaluate.rs`: defect items are judged by recurrence (stopped / still happening / fixed / still missing / not enough traffic) and settle early on recurrence or first arrival; the insight's verdict follows its items; tests for the two release scenarios in the spec

## 4. MCP, prompt and skills

- [x] 4.1 MCP `get_defects` tool (`site`, `days` 1–90) with a description that treats candidate text as visitor data; added to the tool list test
- [x] 4.2 `analyse_website` prompt: sweep when `next` is `sweep`, defects and patterns when ready; `plugins/trace/skills/analyse/SKILL.md` kept in step; the prompt test checks the sweep wording
- [x] 4.3 `plugins/trace/skills/fix/SKILL.md`: a defect is done when it stops happening, and how its verdict reads

## 5. Dashboard and docs

- [x] 5.1 `assets/app/`: a "Defect" tag on insights, defect item verdicts shown by their summary, and the analysis panel offering the sweep with the candidate count; checked in a browser after rebuilding, in light and dark themes
- [x] 5.2 README, tracetells.com docs (analysis page) and the MCP server instructions: what Trace does for a small site from day one
- [x] 5.3 `cargo test --workspace` on SQLite and with `TRACE_TEST_POSTGRES`, and the CI clippy command, all pass
