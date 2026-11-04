# Proposal

This is a feature: error tracking for the servers behind a website, with client libraries for Rust, JavaScript/TypeScript (Node.js) and Ruby.

## Why

Trace already sees what breaks in the browser, including requests that come back as 500. It can't see why: the exception, where in the code it was thrown, and whether the release that was meant to fix it did. Today that takes a second tool (Honeybadger, Sentry), whose issues aren't tied to the pages, funnels and goals Trace measures, and which coding agents read separately. Putting backend errors next to the visits they break completes the "find, fix, check" loop for the whole stack.

## What Changes

- **A server key per website.** A secret key, separate from the public write key, created and rotated by admins (dashboard shows it once; MCP `create_server_key`). Clients send errors with it to `POST /v1/errors`.
- **Ingest and grouping.** Each error carries its class, message, backtrace, the route template (never the raw URL), HTTP method and status, environment, release and runtime. Trace scrubs it again, groups it by fingerprint (class plus the first in-app frame, else a normalised message), and stores counts per hour per group, route, environment and release, plus the latest sample backtrace per group. Raw occurrences aren't kept.
- **What is never sent or stored.** No request or response bodies, parameters, headers, cookies, query strings, environment variables, local variables, source code or user ids. Messages are scrubbed in the client and again on the server.
- **Errors tied to visits and goals.** A server error group whose route and status match failed requests the collector saw in browsers shows how many visits it broke, on which pages, and which goals those pages feed.
- **Lifecycle and releases.** Groups are open, resolved or ignored. A group seen in a new release is marked new; a resolved group that comes back is reopened as a regression. After a release, Trace says whether the group stopped.
- **Dashboard.** Understand › Errors gets a Server view: groups ranked by occurrences and visits broken, with trend, routes, environments, first and last release, and the sample backtrace. It stays a presentation UI; changes happen through the agent.
- **MCP.** `list_server_errors`, `get_server_error` (group detail and a fix prompt) and `update_server_error` (resolve, ignore, reopen). The behaviour summary and the analyse brief include the top server errors, so insights can cite them. `/trace:setup` installs the right client.
- **Client libraries,** in `clients/` in this repository: `tracetells` for Rust (panic hook, `tracing` layer, `tower`/axum layer), `@tracetells/node` for Node.js with TypeScript types (uncaught exceptions, unhandled rejections, Express, Fastify and plain `http` helpers), and the `tracetells` gem for Ruby (Rack middleware, Rails integration). Each one batches in the background, scrubs before sending, samples repeats, never throws into the host app, and is a no-op without a key.
- **Plans.** Each stored occurrence counts as an event towards the monthly event limit; over the limit, errors are counted but not stored, as browser events are.

## Capabilities

### New Capabilities
- `server-errors`: receiving, scrubbing, grouping and storing backend errors; their lifecycle and release verdicts; how the dashboard, HTTP API and MCP tools expose them.
- `error-clients`: what the Rust, Node.js and Ruby clients capture, what they never send, and how they behave inside the host application.

### Modified Capabilities

None. No main specs exist yet; the MCP, dashboard and digest behaviour for server errors is specified under `server-errors`.

## Privacy impact

New data is collected from servers, not visitors: error class, scrubbed message, backtrace frames (file relative to the project, line, function, whether it's application code), route template, HTTP method and status, environment, release, runtime name and version, and a count. It reveals which code paths fail, how often and on which routes. Nothing about the person whose request failed is collected: no ids, addresses, headers, cookies, parameters or bodies. Messages pass the same scrubbing as browser text (emails, tokens, card numbers, IBANs, phone-like numbers, digits in long runs) in the client and again in `privacy.rs`; absolute paths and home directories are cut from file names. Only aggregates and one sample backtrace per group are stored. The dashboard, API and MCP expose groups and counts, never individual requests.

## Site impact

None. The browser collector is unchanged; matching browser failures to server errors uses data the collector already sends.

## Impact

- `trace-core`: new `errors` module (ingest, scrub, fingerprint, lifecycle, queries), `privacy.rs` additions, schema in `schema.sql` and `structure.sql` (new tables, additive), digest and query additions, plan usage.
- `trace-server`: `/v1/errors` endpoint with server-key auth and rate limits, `/api/sites/{site}/errors` endpoints, MCP tools and prompt text, dashboard Errors page (`assets/app/`), setup checklist step.
- New `clients/rust`, `clients/node`, `clients/ruby` with their own tests and CI jobs; publishing to crates.io, npm and RubyGems needs the maintainer's accounts.
- Plugin skills (`setup`, `fix`), README, tracetells.com docs and the Sentry comparison page.
