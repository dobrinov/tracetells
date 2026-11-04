# Tasks

- [x] 1 `readiness.rs`: `projects.analysed_at` (both schemas and migration), `mark_analysed`, `status` with thresholds and a message, and tests
- [x] 2 Record analyses from `add_insights` (an empty list allowed) and Trace's own analysis; the analysis prompt says to save an empty list when nothing is worth saving
- [x] 3 MCP: `get_analysis_status`, and the status in `get_overview`
- [x] 4 `GET /api/sites/{site}/analysis` with `?format=text` and `?format=claude-hook`; the Insights banner; the Setup page's Claude Code hook; checked end to end
- [x] 5 Sign-up links terms and privacy (`TRACE_TERMS_URL`, `TRACE_PRIVACY_URL`); the prompt says who found an insight
- [x] 6 tracetells.com: privacy and terms pages, linked from every footer and the consent bar
- [x] 7 README and deploy guide
