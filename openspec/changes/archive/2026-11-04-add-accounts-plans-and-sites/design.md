# Design

## Context

Exploring the code shows a single-workspace server built around one `projects` row:

- **Storage:** 12 tables in `crates/diagnostic-core/src/schema.sql` are keyed by `project_id`. Behaviour tables (`events`, `sessions`, `event_sources`), configuration (`kpis`, `event_definitions`, `privacy_rules`, `projects.config`) and results (`ideas`, `evaluations`, `analysis_runs`) all hang off the project. So do the people tables: `users`, `invites`, `api_tokens`, `audit_log`, plus `projects.require_mfa`.
- **Server state:** `crates/diagnostic-server/src/main.rs` calls `db::default_project()` once at startup and puts its id in `AppState.project_id`. Every handler in `api.rs`, `auth_api.rs` and `mcp.rs` reads `state.project_id` (about 70 uses). Sessions and tokens are checked against that one project in `auth::session` and `auth::verify_api_token`.
- **Collection:** `/v1/collect` already resolves the project from the write key (`db::project_by_key`), so collection works per project. Privacy rules are compiled once into `AppState.rules`, and the served `d.js` embeds that single rule set.
- **Retention:** `retention::purge` runs hourly with one global `--retention-days`.
- **Analysis:** `diagnostic_agent::analyze` runs Claude whenever a key is set, with no count.
- **Roles:** `auth::Role` has `Viewer < Member < Admin < Owner`. `auth_api::required_role` maps endpoints to roles. Personal API tokens (`TokenAuth`) act with their owner's role or as `Viewer` when read-only.

## Goals / Non-Goals

**Goals:**
- Accounts above websites, with the operator area and plans described in the specs.
- Keep the analytics code (ingest, KPIs, digest, ideas, evaluation, privacy) unchanged apart from receiving a website id.
- Upgrade existing single-workspace databases in place without data loss.

**Non-Goals:**
- A person belonging to more than one account (emails stay unique per server).
- Billing, payments or self-service upgrades (paid plans are arranged by contacting the operator).
- Moving off SQLite. The storage split for the hosted edition is separate work.
- Backend event ingestion (on the roadmap).

## Decisions

### A "project" stays the website
The `projects` table and `project_id` columns keep their names and now mean "website". A new `accounts` table sits above them, and `projects` gains `account_id` and `slug`.
- **Why:** behaviour, configuration and results tables need no change, and most of the 286 uses of `project_id` stay correct. Renaming everything to `site_id` would touch every module for no behavioural gain.
- **Alternative:** a full rename. Rejected as churn; user-facing text says "website" throughout.

### Account-owned tables move to `account_id`
`users`, `invites`, `api_tokens` and `audit_log` get `account_id`, and `require_mfa` moves to `accounts`. `audit_log` also keeps an optional `project_id`, for entries about one website.
- **Migration:** one account per existing project. Each person, invite, token and log entry is copied across by their project, and user emails get a unique index.
- **Alternative:** a join table for multi-account membership. Rejected as a non-goal; a unique email keeps sign-in to one step.

### Request scoping: who, then which website
- **Who:** `AppState.project_id` is removed. The auth middleware resolves a principal with `account_id`, role and operator flag: from the session's user, or from a token's owner.
- **Which website:** routes for website data move under `/api/sites/{site}/...`. A middleware resolves `{site}` to a project within the principal's account, or returns 404 so one account can't detect another's websites, and passes a `Site { id, slug, account_id }` extension to handlers.
- **Account-level routes** stay at `/api/team`, `/api/account`, `/api/audit` and `/api/plan`. **Operator routes** are `/api/operator/*`.
- **The CLI and MCP** resolve the website the same way: from `--site` or a `site` argument, defaulting when the account has exactly one website.
- **Why:** one choke point enforces isolation, so handlers can't forget it.

### Privacy rules and the served collector per website
`AppState.rules` becomes a map from `project_id` to compiled rules, loaded lazily and invalidated on change. Ingest looks up the rules for the project the write key resolves to.
- **Problem:** the collector script is shared (`/d.js`), but the rules differ per website.
- **Choice:** the snippet's URL becomes `/d.js?k=<write key>` and the server embeds that website's rules. Each website has its own cached, precompressed variant, keyed by the rules' hash, and its own ETag.
- **Compatibility:** the old `/d.js` with no `k` keeps working with no workspace rules, and the server still enforces them at ingest.

### Plans as templates plus nullable overrides
- **Tables:** `plans` (id, key, five nullable limits, `is_default`) and `accounts.plan_id`, with five nullable `override_*` columns on `accounts`.
- **Effective limit:** `override ?? template`, where null means unlimited.
- **Seeding:** `unlimited` (all null, default), plus `free`, `friend` and `owner` with starting values the operator can edit in the operator area. `DIAGNOSTIC_DEFAULT_PLAN` can change the default at first start.
- **Alternative:** copying limits into each account. Rejected because template edits must reach existing accounts (specs: Plan templates and limits).

### Usage counting
A `usage(account_id, month, events, events_dropped, ai_runs)` table is updated at ingest and analysis.
- **The events check:** ingest compares the month's count with the limit in memory, from a per-account counter refreshed from the table each minute, so the hot path does no extra reads. It then adds the batch with one upsert.
- **Accuracy:** the cap can be overshot by at most one minute of traffic per server process. That's acceptable for a soft quota and is stated in the docs.
- **Analysis:** checks and increments `ai_runs` in the same transaction that records the run, so the analysis cap is exact.

### Retention per account
`retention::purge` loops over accounts and deletes events older than `min(plan retention, server --retention-days)` for each of the account's websites.

### Roles collapse to owner, admin, user
- **The enum:** `Role` becomes `User < Admin < Owner`.
- **Migration:** stored `member` and `viewer` become `user`, and each former viewer gets an activity log entry.
- **Read-only tokens** stay a token flag, enforced by a separate "read-only principal" check that refuses every write, instead of mapping to a role.
- **`required_role`'s table:** endpoints that needed `Member` now need `User`, and the old `Viewer` reads also need `User`.

### Operator rights
- **Storage:** a `users.is_operator` flag set only by `diagnostic operator grant|revoke <email>`, which writes an audit entry under the actor `cli`. There is no route that writes it.
- **Access:** `/api/operator/*` requires the flag, a session that passed two-step verification, and a user with TOTP enabled. Tokens are always refused.
- **What it returns:** account metadata, people, websites, limits and usage. Never behaviour data: the handlers can only read the `accounts`, `plans`, `usage`, `users` and `projects` tables and the per-project counts.

### Sign-up
`--allow-signup` (`DIAGNOSTIC_ALLOW_SIGNUP`), off by default, enables `POST /api/auth/signup`. Sign-up creates an account on the default plan, its first user as owner, and a first website named after the account. It goes through the same login rate limiter, plus a per-IP limit on sign-ups.

## Risks / Trade-offs

- [**Broken API clients:** API paths and the MCP tool arguments change.] → Single-website accounts keep working without naming a website. The old `/api/<resource>` paths answer for one release with a deprecation header when the account has exactly one website.
- [**Migration bugs could cross data between accounts.**] → The migration runs in one transaction, and tests cover upgrading a populated single-workspace database. A per-account isolation test asserts that every website-scoped endpoint returns 404 for another account's website.
- [**Free sign-ups could be abused for spam or load.**] → Sign-up is off by default. The free plan's monthly event cap and retention bound the load, plus a per-IP rate limit on sign-up.
- [**Monthly event caps can be exceeded slightly.**] → Documented as a soft limit with at most one minute of overshoot.
- [**Former viewers gain write access.**] → The migration records each change in the activity log, and the README release notes tell admins to review roles after upgrading.

## Migration Plan

1. In one transaction:
   1. Create `accounts`, `plans` and `usage`; seed the plans; create one account per project, on the default plan.
   2. Add `account_id` to `users`, `invites`, `api_tokens` and `audit_log`, and fill it from the project; move `require_mfa` across.
   3. Remap the roles.
   4. Add the unique email index. If two projects share an email, the migration stops and names the conflicting addresses for the operator to resolve.
2. Snippets with `/d.js` keep working. The Setup page shows the new `/d.js?k=` snippet and explains that workspace rules then also apply in the browser.
3. **Rollback:** restore the pre-upgrade database file, which the server copies to `diagnostic.db.pre-accounts` before migrating.

## Open Questions

- Starting limit values for `free`, `friend` and `owner`. The operator can change them at any time without code changes, so the defaults are a guess, e.g. free: 7 days, 1 website, 100,000 events, 3 members, 5 analyses.
- The "ask for more" contact shown on Plan & usage: an address or link from the operator, set as `DIAGNOSTIC_CONTACT_URL`.
