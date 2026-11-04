# Design

## Context

Browser errors already exist. The collector (`crates/trace-server/assets/d.js`) reports exceptions, rejections, console errors, failed requests and missing resources as `events` rows of type `error`: `name` is the kind (`exception`, `rejection`, `console`, `network`, `resource`), `text` the scrubbed message, `selector` the source location, `path` the page. A failed request's `text` reads `GET /api/reviews/123 → 500`, with the query string removed and the path shortened by `shortUrl`. `privacy::redact_text`, `redact_path` and `one_line` (`crates/trace-core/src/privacy.rs`) scrub every field again at ingest (`ingest.rs`). `digest.rs` groups errors by kind, message and page into `ErrorStat`, and the dashboard's Errors page (`assets/app/pages.js`) and the analyst summary read that.

Ingest is `POST /v1/collect` in `api.rs`. It authenticates with the website's public write key, rate-limits by address, counts events in `usage.rs` against `monthly_events` (`plans.rs`), and drops batches from local origins once a site is live. Every events row belongs to a session. Server errors have no session, and a public key would let anyone fake them.

Releases exist as insight releases (`POST /api/sites/{site}/releases`, `evaluate.rs`): a KPI's 14 days before and after with a Welch test. There is no notion of a deployed version.

MCP tools live in `crates/trace-server/src/mcp.rs`. Plugin skills are in `plugins/trace/skills/`, and the MCP prompts are built from them.

There is no `clients/` directory, and the workspace has only `trace-core` and `trace-server`.

## Goals / Non-Goals

**Goals:**
- One shape of error payload for all three clients, documented as a small JSON schema the server validates.
- Storage that stays aggregate: counts plus one sample backtrace per group, on SQLite and Postgres.
- Matching server groups to browser failures without any per-request or per-person link.

**Non-Goals:**
- Performance tracing, logs, breadcrumbs, request context or user feedback widgets.
- Alerts and notifications. Trace sends no email today. A later change can add a webhook.
- Source maps and symbolication, uploads of source or debug files.
- Mobile, browser (d.js covers it), Python, Go or other runtimes in this change.
- Publishing the packages to registries as part of the code. That is a manual step for the maintainer.

## Decisions

**A separate ingest endpoint and a secret key.** `POST /v1/errors` with `Authorization: Bearer tse_…`. Keys are stored hashed (like API tokens in `auth.rs`), one active per website, in a new `server_keys` table. *Alternative:* reuse the write key on `/v1/collect`. Rejected because write keys are public in every page, so anyone could flood a site with fake server errors and spoil its analysis. *Alternative:* personal API tokens. Rejected because they act as a person and carry dashboard rights a server doesn't need.

**Aggregate storage.** New tables, mirrored in `schema.sql` and `structure.sql`:
- `error_groups`: id, project, fingerprint (unique per project), class, message, status (`open|resolved|ignored`), first and last seen, first and last release, resolved_at, resolved_in_release, resolved_by, regressed_in_release, the sample (JSON: frames, route, method, status, runtime) and when it was sampled.
- `error_counts`: group, hour (epoch ms floored), route, method, status, environment, release, count. Primary key over all but count, upserted with `ON CONFLICT … DO UPDATE SET count = count + excluded.count`, which works on both databases.

*Alternative:* a `notices` table of every occurrence, as Honeybadger and Sentry keep. Rejected because it is per-request history, against the aggregates-only rule, and it grows without bound. Retention deletes `error_counts` rows older than the plan's retention, and groups with no counts left.

**Fingerprints.** SHA-256 over the class and the first in-app frame's file and function, without line numbers so they survive small edits. Without an in-app frame, the class and the message after normalisation: digits become `#`, quoted strings `'…'`, and hex or uuid ids `:id`. Clients can't set a fingerprint, so they can't split or merge groups by accident.

**Scrubbing on both sides.** The server reuses `privacy::redact_text` (strict) for the class and message, `redact_path` for routes, and a new `redact_frame` that keeps the path from the first of `src/`, `app/`, `lib/` or the project root marker and drops anything before it, including home directories. Clients carry a copy of the same patterns, with shared test vectors in `clients/test-vectors.json` that each client's tests and `privacy.rs`'s tests read, so the four stay in sync.

**Matching browser failures.** At query time, Trace parses `network` errors' text (`METHOD path → status`), turns the path into a route with `redact_path` and the site's `routes`, and joins on method, route and status over the same window. The result is the number of sessions with that failure and their pages. No identifier passes between browser and server. *Alternative:* a request id header added by the collector. Rejected because it would link a person's visit to a server log line.

**Lifecycle and verdicts.** On ingest, a group whose status is `resolved` and that receives an occurrence from a release other than `resolved_in_release`, or any occurrence 1 hour after `resolved_at` when releases aren't sent, reopens with `regressed_in_release`. The verdict compares occurrences per day over the 14 days before resolving with those since. It says "stopped" with zero since and at least 7 days elapsed, otherwise "dropped by N%" or "still happening". It is computed in the existing evaluation pass (`evaluate.rs`), daily.

**Usage.** Stored occurrences add to the same `usage` counter as browser events. Each error in a batch counts its `count`, capped at 1,000 per error, so a storm can't use up a month in one request.

**Clients.**
- **Rust:** crate `tracetells` in `clients/rust`. It is a workspace member, so CI builds and tests it, but it doesn't depend on `trace-core` and stays light: `ureq` with rustls, `serde_json` and a background thread. It offers `std::panic::set_hook` chaining, a `tracing_subscriber::Layer` behind the `tracing` feature, and `tower::Layer` behind the `tower` feature, which records the axum `MatchedPath`.
- **Node.js:** package `@tracetells/node` in `clients/node`, written in TypeScript and shipped as compiled ESM plus CJS with types. It has no runtime dependencies and uses `fetch` (Node 18+). Helpers: `expressErrorHandler()`, `fastifyPlugin`, `wrapHandler()` for plain `http`. The route comes from `req.route.path` (Express) or `routeOptions.url` (Fastify).
- **Ruby:** gem `tracetells` in `clients/ruby`. It has no runtime dependencies (`net/http`, a background thread with a `Thread::Queue`). It provides `Tracetells::Rack` middleware, and in Rails a Railtie that subscribes to `Rails.error` (7.0+) and reads the route from `request.route_uri_pattern` (7.1+) or the journey route.

Each client sends at most once per second, at most 100 errors per batch, from a 1,000-error queue, and folds repeats within 60 seconds into a count.

**Dashboard and MCP.** The Errors page gets two tabs, Browser (as today) and Server, backed by `GET /api/sites/{site}/errors` and `/errors/{id}`. New MCP tools are `list_server_errors`, `get_server_error`, `update_server_error` and `server_key` (status, create, rotate, revoke). The digest gains `server_errors`, the top 15 open groups with visits broken, and the analyse prompt names it. The setup checklist gains an optional "Server errors" step that ticks when the first error arrives.

## Risks / Trade-offs

- [A message can contain a name or free text that patterns miss] → Messages are cut to 300 characters. Trace stores one representative message per group, not every variant. README tells users to keep personal data out of exception messages, and the server's privacy rules (`redact_pattern`) apply to server errors too.
- [Matching browser failures to server routes by text parsing is approximate] → It is shown as "visits with this failure" and never feeds the ranking alone. Unmatched groups rank by occurrences.
- [Three clients in three ecosystems to maintain] → They share test vectors and one payload schema. Each is small and dependency-free, and has its own CI job.
- [Errors from local development pollute production numbers] → `environment` is stored, and the dashboard defaults to `production`, or the most common environment when none is called that.
- [A leaked server key lets someone send fake errors] → Rotation is one command, and rate limits per key cap the damage. Keys can only send, never read.

## Migration Plan

Additive only: new tables are created by `db::migrate` on start, and nothing changes for sites without a server key. Rollback is the previous image; the new tables are ignored. The clients are released after the server endpoint is deployed.

## Open Questions

- Package names on registries (`tracetells`, `@tracetells/node`) depend on what is free when publishing. The code can be renamed before the first release without changing the specs.
