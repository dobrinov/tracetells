# Tasks

- [x] 1 Schema: `insights.released_definition`, `released_direction`, `evaluations.settled_at` (both schemas and the migration); `Verdict::Measuring`
- [x] 2 `set_status` takes a release time (validated), snapshots the KPI and drops a stale evaluation; `set_kpi` does the same for released insights
- [x] 3 Evaluation over a fixed 14-day horizon: measuring, then settled; judged by the snapshot; overlapping releases named; tests
- [x] 4 `parse_time` and `date` with tests
- [x] 5 `released_at` in the API, MCP, CLI (`--at`) and dashboard; `POST /releases` for deploys, with a test; `Trace-Insight` trailer in task prompts
- [x] 6 Demo releases 14 days ago; README "Did it help?" with a CI snippet
