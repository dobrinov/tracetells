# Tasks

## 1. Storage and migration

- [x] 1.1 Add `accounts`, `plans` and `usage` tables, and add `account_id`/`slug` to `projects` and `account_id` to `users`, `invites`, `api_tokens` and `audit_log` in `schema.sql`, with the migration in `db::migrate`; verify a fresh database has the tables and indexes with `sqlite3 .schema`
- [x] 1.2 Migrate an existing single-workspace database in one transaction:
  - copy `diagnostic.db.pre-accounts` first
  - create one account per project
  - backfill `account_id`
  - move `require_mfa`
  - remap `member`/`viewer` to `user`, recording each viewer's change in the activity log
  - stop with the conflicting addresses if emails collide

  Verify with a test that upgrades a populated database and checks people, tokens, settings, data and the write key survive.
- [x] 1.3 Seed the plan templates `unlimited` (default, no limits), `free`, `friend` and `owner`, honouring `DIAGNOSTIC_DEFAULT_PLAN` at first start; verify with a test that a new server's first account is on `unlimited`

## 2. Accounts and roles

- [x] 2.1 Move users, invites, tokens and the activity log to the account in `auth.rs`, with unique emails across the server; verify an invite for an address used in another account is refused
- [x] 2.2 Replace `Role` with `User < Admin < Owner`, keep read-only as a token flag enforced as "no writes", and update `required_role`; verify the existing role and token tests pass, plus a test that a read-only token is refused for every write route
- [x] 2.3 Add `--allow-signup` and `POST /api/auth/signup`: an account on the default plan, its owner and a first website, with per-IP rate limiting. Verify that sign-up off refuses with the invitation message, and sign-up on creates the account on the default plan.
- [x] 2.4 Update the Team page, Account page and README for the owner/admin/user roles and sign-up; verify in the browser that no "viewer" or "member" wording remains

## 3. Websites

- [x] 3.1 Resolve the principal's account in the auth middleware, and add a website middleware for `/api/sites/{site}/...` that returns 404 for other accounts' websites. Remove `AppState.project_id`. Verify with a cross-account isolation test that every website route returns 404 for another account's website.
- [x] 3.2 Move website routes (ideas, KPIs, digest, config, privacy, runs, analyze, evaluate, collection status) under `/api/sites/{site}`, keeping the old paths for single-website accounts with a `Deprecation` header; verify both forms work for a single-website account and the old form is refused when there are two
- [x] 3.3 Add and remove websites (slug, write key, plan check), and `diagnostic site add|list|remove`; verify adding beyond the website limit is refused and removal deletes only that website's data
- [x] 3.4 Make privacy rules per website in `AppState`, and serve `/d.js?k=<write key>` with that website's rules and its own ETag, falling back to rule-less `/d.js`; verify two websites' served collectors embed different rules and both pass `node --check`
- [x] 3.5 Add `--site` to `diagnostic push`, `check` and `prompt`; add a `site` argument to MCP tools, defaulting when there is one website and otherwise erroring with the list; add a `list_sites` tool. Verify with MCP tests for the single-website default, the ambiguous error and `list_sites` scoping.
- [x] 3.6 Add a website switcher, an Add website flow and per-website Setup snippets (with `?k=`) to the dashboard, and update the README and Setup copy; verify in the browser that switching websites changes every page's data

## 4. Plans and limits

- [x] 4.1 Compute effective limits (`override ?? template`, null = unlimited) in a `plans` module; verify unit tests for inheritance, overrides and removal of an override
- [x] 4.2 Enforce monthly events at ingest with an in-memory per-account counter flushed to `usage`. Drop excess events while still returning 204, and count them. Verify with a test that events beyond the limit aren't stored and are counted in `events_dropped`.
- [x] 4.3 Enforce Claude analysis runs in `analyst.rs`: fall back to heuristics with a message under auto, refuse an explicit Claude request, and never limit heuristics. Verify with tests for all three paths.
- [x] 4.4 Enforce website and member limits, with pending invites counted and lowering a limit never deleting anything; verify with tests at, below and above each limit
- [x] 4.5 Run per-account retention, `min(plan, --retention-days)`, in the hourly job; verify with a test that a 7-day account loses 8-day-old events while an unlimited account keeps them
- [x] 4.6 Add the Plan & usage page (limits, this month's usage, events not recorded, the `DIAGNOSTIC_CONTACT_URL` contact) with no plan names, and a dashboard banner when events are being dropped; verify in the browser as a `friend` account that limits show and the word "friend" does not

## 5. Operator area

- [x] 5.1 Add `users.is_operator` and `diagnostic operator grant|revoke|list <email>`, audited under the actor `cli`; verify that granting works only from the CLI and that no route writes the flag (route list test)
- [x] 5.2 Add `/api/operator/*`: accounts with plan, limits, overrides and usage; account detail with people and websites; plan template editing and the default plan; per-account plan and overrides; account creation returning an owner invite link. Require operator + two-step and refuse tokens. Verify a non-operator gets 403, an operator without two-step is asked to enrol, and responses contain no behaviour data, KPIs or ideas.
- [x] 5.3 Add the operator Admin area to the dashboard (accounts list, account detail, plans editor, create account), visible only to operators, with README docs; verify in the browser as owner@example.com after `diagnostic operator grant`

## 6. Demo and integration

- [x] 6.1 Update `diagnostic demo` to seed:
  - the operator's account on `owner` with two websites
  - a friend's account on `friend`
  - a free account
  - owner@example.com granted operator rights

  Verify `diagnostic demo` then signing in shows the Admin area and the website switcher.
- [x] 6.2 Run the full suite, a cross-account leak check (every API route and MCP tool with another account's token) and the collector privacy leak test against two websites; verify there are no failures and no cross-account data

## Workflow follow-up

- Archive this change with `/opsx:archive` once implemented, so `accounts`, `sites`, `plans` and `operator-admin` become the repository's first main specs.
