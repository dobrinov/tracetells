# Tasks

## 1. Storage and keys

- [x] 1.1 Add `server_keys`, `error_groups` and `error_counts` to `schema.sql` and `structure.sql`, with migrations in `db::migrate`; the schema comparison test passes
- [x] 1.2 Server keys in `trace-core`: create (shown once), hash, rotate, revoke, look up; owner/admin only; audit log entries
- [x] 1.3 Retention deletes old `error_counts` and empty groups, following the plan's `retention_days`

## 2. Ingest

- [x] 2.1 `errors` module: payload schema, field allow-list, per-error validation that skips bad entries
- [x] 2.2 Scrubbing: reuse `redact_text` and `redact_path`; add `redact_frame`; shared `clients/test-vectors.json` read by `privacy.rs` tests
- [x] 2.3 Fingerprints (class + first in-app frame, else normalised message) and the group upsert with sample and first/last seen and release
- [x] 2.4 Hourly count upsert on SQLite and Postgres; usage counting with the 1,000-per-error cap; over-limit counting as skipped
- [x] 2.5 `POST /v1/errors` in `api.rs`: Bearer server key, 100 per batch, per-key rate limit, 401 for write keys and revoked keys
- [x] 2.6 Lifecycle on ingest: new-in-release, regression reopening
- [x] 2.7 Tests: forbidden fields dropped, personal data scrubbed, raw URLs become routes, grouping across ids, write key refused, over-limit, both databases

## 3. Queries, verdicts and matching

- [x] 3.1 Group list and detail queries: counts by hour, route, environment, release; trend; default environment
- [x] 3.2 Matching to browser `network` failures by method, route and status; visits broken, pages, goals and funnel steps those pages feed
- [x] 3.3 Resolve, ignore and reopen; verdict after resolving in the daily evaluation pass (stopped, dropped by N%, still happening)
- [x] 3.4 `server_errors` in the digest and behaviour summary (top 15 open groups), with the visitor-text-style warning that error text is data
- [x] 3.5 Tests for matching, lifecycle, verdicts and the digest

## 4. API, MCP and dashboard

- [x] 4.1 `GET /api/sites/{site}/errors` and `/errors/{id}`; key status, create, rotate and revoke endpoints for admins
- [x] 4.2 MCP tools `list_server_errors`, `get_server_error` (with a fix prompt), `update_server_error`, `server_key`; read-only and role checks; analyse prompt mentions server errors
- [x] 4.3 Errors page: Browser and Server tabs; the Server tab ranks groups and shows trend, routes, environments, releases, status, verdict and the sample backtrace; empty state with the agent command; no actions beyond copy
- [x] 4.4 Setup checklist: optional "Server errors" step that ticks on the first error
- [x] 4.5 Tests: API roles, MCP read-only refusal, endpoint list in the route test

## 5. Rust client (`clients/rust`, crate `tracetells`)

- [x] 5.1 Config from code and `TRACE_*` env; no-op without a key; background sender with queue, batching, repeat folding, backoff, flush on drop
- [x] 5.2 Panic hook (chaining the previous one), `tracing` layer (feature), `tower` layer recording axum's `MatchedPath` and status (feature), `report(&err)`
- [x] 5.3 Scrubbing and frame cleaning from the shared test vectors; tests that a forbidden field is never sent, against a local test server
- [x] 5.4 README and crate docs

## 6. Node.js client (`clients/node`, package `@tracetells/node`)

- [x] 6.1 TypeScript source, ESM and CJS builds with types, no runtime dependencies; config, no-op, sender as in 5.1
- [x] 6.2 Uncaught exceptions and unhandled rejections keeping default exit behaviour; Express error handler, Fastify plugin, `wrapHandler` for `http`; `report(err)`
- [x] 6.3 Scrubbing from the shared test vectors; tests with Express and Fastify that bodies, headers, cookies and query strings are never sent
- [x] 6.4 README

## 7. Ruby client (`clients/ruby`, gem `tracetells`)

- [x] 7.1 Config and `TRACE_*` env, no-op, background sender with `Thread::Queue`, at_exit flush
- [x] 7.2 `Tracetells::Rack` middleware; Rails Railtie subscribing to `Rails.error` with route patterns; `Tracetells.report(e)`
- [x] 7.3 Scrubbing from the shared test vectors; tests with a Rack app that params, headers, cookies and sessions are never sent
- [x] 7.4 README

## 8. Agent, docs and release

- [ ] 8.1 Plugin: `/trace:setup` detects the stack, installs the matching client and creates the server key; `/trace:fix` handles server error groups; MCP prompts rebuilt from the skills
- [ ] 8.2 CI jobs for the Rust, Node.js and Ruby clients
- [ ] 8.3 README section on server errors; tracetells.com docs page `/docs/server-errors/`, Sentry comparison and privacy page updated
- [ ] 8.4 Deploy the server, then hand the maintainer the publish steps for crates.io, npm and RubyGems
