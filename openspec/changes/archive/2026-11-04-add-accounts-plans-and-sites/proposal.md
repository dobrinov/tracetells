# Proposal

**Kind:** feature.

## Why

Today one Diagnostic server equals one website and one team. The hosted edition has to serve many customers: a company needs several websites under one account, and each account needs limits that match how it came in (a random tester, one of the operator's friends, the operator himself). The operator needs one place to see and manage all of that.

## What Changes

- **Accounts.** An account (a company or one person) owns its websites, its people, their roles, the two-step verification policy, API tokens, the activity log and a plan. On upgrade, the existing workspace becomes the first account.
- **Several websites per account.** Each website has its own write key, `diagnostic.json` (KPIs and events), privacy rules, ideas, evaluations and collection status. The dashboard gets a website switcher and an "Add website" flow. The HTTP API, `diagnostic push` and MCP act on one website, chosen explicitly.
- **Plans.** Every account is on a plan created from a template. A plan sets five limits:
  - data retention in days
  - websites per account
  - events per month
  - members per account
  - Claude-powered analysis runs per month (heuristic analysis is never limited)

  Any limit can be overridden for a single account, and an empty limit means no limit. Built-in templates are `free`, `friend`, `owner` and `unlimited`. Plan names are internal and never shown to customers, who only see their limits and usage.
- **Default plan per deployment.** New accounts get the deployment's default plan: `unlimited` unless configured otherwise, so the community edition has no limits. A hosted deployment sets `free`.
- **Sign-up.** A deployment may allow self-service sign-up, which creates a new account on the default plan. It is off by default. The operator can also create an account on a chosen plan, for example `friend`, and send its owner an invite link.
- **Operator admin area.**
  - **Access:** for people flagged as operators. The flag is granted only on the server with `diagnostic operator grant <email>`, never from the web, and operators must use two-step verification.
  - **What it shows:** every account, with its people and websites, plan, overrides and this month's usage.
  - **What it can do:** create accounts, edit plan templates, change an account's plan or overrides, and grant or remove operator rights.
- **BREAKING: account roles are `owner`, `admin` and `user`.**
  - **`member` becomes `user`.**
  - **The `viewer` role for people is removed.** Existing viewers become users, and the change is recorded in the activity log.
  - **Read-only access** stays available as an option on personal API tokens.
- **BREAKING: per-website endpoints are scoped by website.** Dashboard API paths for website data take the website: `/api/sites/{site}/...`. `diagnostic push` takes `--site`, and MCP tools take a `site` argument. Every one of these defaults to the account's only website when it has just one.

## Capabilities

### New Capabilities
- `accounts`: accounts as the owner of people, roles, two-step policy, API tokens and the activity log; sign-up and account creation.
- `sites`: several websites per account, each with its own collection key, configuration, privacy rules and results; website selection in the dashboard, API, CLI and MCP.
- `plans`: plan templates, per-account overrides and how each limit is enforced and shown.
- `operator-admin`: the internal operator area and how operator rights are granted.

### Modified Capabilities

## Privacy impact

- **Visitors' behavioural data:** collection and scrubbing don't change. Each website's privacy rules stay separate, and no website or account can read another's data. Events above a monthly limit are dropped, not stored.
- **What operators see:** account metadata, people (email, name, role, two-step status, last sign-in), websites (address, collection status) and usage counts. Operators do **not** get access to any account's analytics, ideas or behavioural data. This keeps "patterns, not people" true for the operator as well.
- **Personal data of account members:** email addresses are now unique across the deployment, and they stay only in the accounts, operator and authentication tables.
- **Retention:** per-plan retention deletes raw events sooner on restricted plans, and never later than the server-wide retention setting.

## Site impact

None. The collector is unchanged, and it still gets `204 No Content` when events are dropped for a limit, so visitors' browsers do no extra work and never retry.

## Impact

- **Storage:** new `accounts`, `plans` and monthly `usage` tables. Users, invites, API tokens, the two-step policy and the activity log move from the project (now a website) to its account. `projects` gains `account_id` and a `slug`. The migration creates one account per existing project.
- **Code:**
  - `diagnostic-core` gains `accounts`, `plans` and `usage` modules, and `auth`, `ingest`, `retention`, `config`, `privacy` and the analyst change.
  - `diagnostic-server` gets website scoping in `api.rs`, `auth_api.rs` and `mcp.rs`, new operator routes, and the `diagnostic operator` and `diagnostic site` CLI commands.
  - The dashboard gets a website switcher, an Add website flow, a Plan & usage page and the operator Admin area.
- **APIs:**
  - **Changed:** dashboard API paths for website data (BREAKING) and MCP tool arguments, which gain `site`.
  - **New:** `/api/operator/*`.
  - **Unchanged:** the collector endpoint `/v1/collect`.
- **Docs:** README sections on accounts, plans, operators and multi-site.
