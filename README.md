# Trace

**Trace**: web analytics, and a way to improve your website's UX, for the agentic era.

Trace collects how people use your product, an analyst agent turns that into specific improvement insights tied to your KPIs, you copy an insight as a prompt for your coding agent, and once it ships Trace measures whether it worked.

```
collect ──▶ digest ──▶ analyst ──▶ insights ──▶ prompt ──▶ ship ──▶ evaluate
 (d.js)    (SQLite)   (Claude or   new → selected → in progress → released
                       heuristics)                      ╰── dismissed (with a reason the analyst learns from)
```

## Try it in a minute

```sh
cargo build --release
./target/release/trace --db demo.db demo                  # demo accounts, plus 30 days of traffic for "Stride", a demo shoe shop
./target/release/trace --db demo.db --site stride analyze  # create insights
./target/release/trace --db demo.db --site stride serve --demo-site 127.0.0.1:4319 \
    --config crates/trace-server/assets/shop/trace.json
```

Open the dashboard at http://127.0.0.1:4318 and sign in with the password `1` (demo accounts only: real passwords need at least 10 characters):

| Sign in as | Account | To see |
|---|---|---|
| `owner@example.com` | Stride Labs, two websites (`stride`, `stride-docs`) | The owner role, the website switcher, and the operator **Admin** area (after turning on two-step under Account) |
| `admin@example.com`, `user@example.com` | Stride Labs | The admin and user roles |
| `friend@example.com` | Bolt Bikes | A limited plan: limits and usage under **Plan** |
| `free@example.com` | Tryout | The tightest plan: one website, 30 days of data |

 Then open the Stride shop, instrumented with the collector, at http://127.0.0.1:4319. The shop has four planted problems: product images that look clickable but aren't, a "Pay now" button that takes 3 seconds with no feedback, a pricing toggle that throws, and a signup form that wipes itself on a bad phone number. Your clicks show up in the dashboard next to the simulated history.

## Install on a server

```sh
cargo build --release
./target/release/trace init     # choose where Trace keeps its data
./target/release/trace serve    # prints a one-time link for creating the first account
```

`trace init` asks which database to use, checks that it works, creates Trace's tables and saves the choice as `TRACE_DB` in `trace.env`. Every `trace` command reads that file from the folder it runs in (or the file named by `TRACE_ENV_FILE`); real environment variables win over it.

| | When | `TRACE_DB` |
|---|---|---|
| **SQLite** (the default) | Trying Trace, development, a few small websites. Nothing to install: one file next to the server. | `trace.db` |
| **Postgres** (recommended for production) | A server other people rely on. Writes don't queue behind a single writer, and you get Postgres's backups and replication. Needs Postgres 10 or newer. | `postgres://trace:PASSWORD@localhost/trace` |

For Postgres, create an empty database and a user that owns it first; Trace adds new columns itself on upgrade:

```sh
createuser trace --pwprompt && createdb trace --owner trace
trace init --database postgres --db postgres://trace:PASSWORD@localhost/trace
```

To run it in Docker instead, see the `Dockerfile` and [deploy/README.md](deploy/README.md). Without a terminal (scripts, provisioning), pass `--database sqlite|postgres` and `--db`. `trace init` won't replace a different database already saved in `trace.env` unless you confirm or pass `--force`, and data isn't copied between the two: a new database starts empty. On start, `trace serve` logs which database it uses (never the password) and warns when a server reachable from outside runs on SQLite; the operator **Admin** area shows the same advice. With systemd, point `EnvironmentFile=` at `trace.env`. Back up SQLite with Litestream or `sqlite3 trace.db ".backup backup.db"`, and Postgres with its own tools.

## First run

A new account opens on **Get started**, a checklist for its first website that ticks itself off as Trace sees each step happen:

1. **Your website**: its name and short name (`--site`).
2. **Install the collector**: the snippet for your kind of site, or "copy as a prompt for my agent". It turns green when the first event arrives.
3. **Check what's protected**: what Trace never records. An admin confirms it, or adds a rule of their own.
4. **Define KPIs in code**: an example `trace.json`, plus `trace check` and `trace push`.
5. **Connect your coding agent**: the MCP command. It ticks off once a personal API token is used.
6. **Invite your team** (optional).

The navigation shows how far along you are until the five required steps are done; after that, the checklist stays linked from Setup. `GET /api/sites/{site}/onboarding` returns the same steps.

## Pieces

| | |
|---|---|
| **Collector** (`/d.js`) | One script tag. Captures pageviews (single-page apps and full reloads), clicks, rage clicks, dead clicks, errors, scroll depth, time on page, form starts and submits, plus custom events from `trace.track(name, props)` or `data-trace-event="name"` markup. It never records input values. |
| **KPIs as code** | KPIs and the custom-event catalogue live in `trace.json` in your repository, next to the code that sends the events. `trace check` validates it, `trace push` publishes it (from CI, say), and the dashboard shows results and tracking health, but never edits definitions. |
| **Analyst** | Builds an aggregated digest and asks Claude for insights through structured output. It sends your KPIs plus existing and dismissed insights so it doesn't repeat itself. Without an API key, or when Claude fails, built-in heuristics take over. |
| **Prompts** | Each insight is rendered as a self-contained task for a coding agent: problem, evidence, goal KPI, proposal, routes and acceptance criteria. Copy it from the dashboard, or run `trace prompt 12 \| claude`. |
| **Errors** | Five kinds: uncaught exceptions, unhandled promise rejections, `console.error` calls (where React, Vue and Angular report the errors they catch), failed `fetch`/XHR requests (status ≥ 400 or no response) and scripts, stylesheets or images that fail to load. |
| **Evaluation** | For released insights, compares the KPI in equal windows before and after the release (Welch test, at least 30 sessions per side) and gives a verdict: improved, regressed, no change or insufficient data. It runs every hour while the server is up. |

## KPIs and events in code

Put a `trace.json` at the root of your application's repository:

```json
{
  "product": "Stride",
  "events": {
    "add_to_cart": "A shopper added a pair of shoes to the cart",
    "purchase": "Payment succeeded"
  },
  "kpis": {
    "checkout_conversion": {
      "name": "Checkout conversion",
      "conversion": { "from": { "pageview": "/checkout" }, "to": { "event": "purchase" } },
      "goal": "increase",
      "target": "55%"
    },
    "errors": { "name": "Sessions with errors", "session_rate": { "error": "*" }, "goal": "decrease", "target": "1%" },
    "pay_rage": { "per_session": { "rage_click": "button#pay", "path": "/checkout" }, "goal": "decrease" }
  }
}
```

- **`events`** is the catalogue of custom events your app sends, with what each one means. The analyst reads these descriptions, and the dashboard's tracking health shows whether each event is arriving, as well as events that arrive without being declared.
- **`kpis`** are keyed by a stable id. Each uses one measure:
  - `conversion`: of the sessions that do `from`, the share that later do `to`
  - `session_rate`: the share of sessions with a matching event
  - `per_session`: matching events per session

  Each KPI also takes a `goal` (`increase` or `decrease`) and an optional `target` (`"55%"` or `0.55`).
- **Event matchers**: `{ "pageview": "/checkout" }` matches a path, `{ "click": "button#pay" }` a selector, `{ "event": "purchase" }` a custom event, and `{ "error": "*" }` any error. Shorthands can add `path` or `selector`, and the full form `{ "type", "path", "selector", "name" }` also works. A pattern ending in `*` matches by prefix.

Send the events from code with `trace.track("purchase", { plan })`, or with no JavaScript at all: `<button data-trace-event="add_to_cart" data-trace-props='{"sku":"S-1"}'>`. On a `<form>`, the event fires when it's submitted.

Then validate and publish:

```sh
trace check                     # validate; non-zero exit on errors, so it can gate CI
trace push --server https://trace.example.com --token "$TRACE_ADMIN_TOKEN"
```

`check` reports every problem with its location, e.g. `kpis.checkout.conversion.from: unknown event type "pagview"`. `push` records the git commit it came from. KPIs are matched by key, so editing one keeps its history and the insights linked to it. A KPI removed from the file is archived; released insights that targeted it are still evaluated. When self-hosting, `trace serve --config trace.json` applies the file at startup and again whenever it changes.

## MCP: use Trace from your coding agent

Trace is also an MCP server, so an agent such as Claude Code or Cursor can run the whole loop: read the KPIs, pick an insight, fetch it as a task prompt, implement it and mark it released, after which Trace measures the impact. Like the dashboard, it only exposes aggregated patterns, never individual sessions.

| Tool | |
|---|---|
| `get_overview` | Collection status, KPIs vs targets, insights per status |
| `get_behaviour_summary` | The aggregated digest the analyst reads |
| `list_kpis`, `get_tracking_health` | KPI values; whether declared events are arriving |
| `list_insights`, `get_insight`, `get_insight_prompt` | Find work, and get it as a complete implementation task |
| `update_insight_status` | Select, start, release or dismiss an insight |
| `run_analysis`, `evaluate_releases` | Create insights; measure released ones |
| `check_config` | Validate a `trace.json` being edited |
| `get_privacy_rules` | What's protected (read-only) |
| `list_sites` | The account's websites |

Every tool except `list_sites` and `check_config` takes a `site` (the website's short name, e.g. `stride`). It can be left out when the account has a single website. Read-only tokens can only use the reading tools.

Agents can't change privacy rules or people, which stay in the dashboard, or KPIs, which stay in `trace.json`. Status changes and analysis runs appear in the activity log under the token's name.

**Remote** (any server; Streamable HTTP, authenticated with a personal API token from **Account**):

```sh
claude mcp add --transport http trace https://trace.example.com/mcp \
    --header "Authorization: Bearer trc_..."
```

Other clients: `{"mcpServers": {"trace": {"url": "https://trace.example.com/mcp", "headers": {"Authorization": "Bearer trc_..."}}}}`

**Local** (on the machine with the database; stdio):

```sh
claude mcp add trace -- trace --db /path/to/trace.db mcp --account "Acme"
```

## Accounts and access

Works for one person or a whole company. An **account** is a company or a solo user: it has people, roles, a two-step policy, API tokens, a plan, and one or more **websites**. Each website has its own write key, `trace.json` (KPIs and events), privacy rules, insights and collection status. Nobody in one account can see anything of another.

- **Getting started alone:** the first time `trace serve` starts, it prints a one-time setup link. Open it to create the first account and its owner. (Or run `trace user create --email you@example.com` on the server.) The link stops working once the owner exists, so whoever reaches a fresh server first can't claim it.
- **More accounts:** `trace serve --allow-signup` lets anyone create an account of their own from the sign-in page (rate-limited per address). Without it, only an operator can create accounts (see below).
- **Teams:** admins invite people from the **Team** page by role and share the invite link themselves. Trace sends no email. Each link works once and expires in 7 days. An email address belongs to one account on a server.

  | Role | Can |
  |---|---|
  | Owner | Everything, including adding and removing websites (one per account, can't be removed) |
  | Admin | Manage the team, two-step policy and privacy rules; see and revoke anyone's API tokens |
  | User | Move insights through their statuses, link KPIs, run analysis |

  Read-only access is for API tokens, not people: make a token read-only when an agent or dashboard should only look.

- **Two-step verification (TOTP):** anyone can add an authenticator app (1Password, Google Authenticator, Authy…) under **Account**. Setting it up gives 10 single-use recovery codes. Admins can **require it for everyone**: anyone without it is held at a setup screen the next time they sign in, and an admin can only turn the requirement on after enabling it for their own account. If someone loses their phone, an admin can reset their two-step verification, and a locked-out owner can use `trace user reset-mfa --email …` or `trace user reset-password --email …` on the server.
- **API tokens are personal.** Everyone creates their own under **Account**, for MCP agents and CI.
  - A token acts with its owner's *current* role, or only reads if it's made read-only. Demoting someone demotes their tokens at once, and removing them revokes their tokens.
  - Tokens can't manage people, tokens or accounts.
  - The activity log shows who acted, e.g. `ana@acme.example via token "Claude Code"`.
  - Admins see and revoke everyone's tokens on **Team**.
  - Pass a token to `trace push --token` (or `TRACE_TOKEN`).
  - The old shared `TRACE_ADMIN_TOKEN` still works but is deprecated, because it belongs to nobody.
- **How it's protected:**
  - Passwords are hashed with Argon2id. Sessions, invite links, API tokens and recovery codes are stored only as SHA-256 hashes, so a copy of the database can't be used to sign in.
  - Session cookies are `HttpOnly`, `SameSite=Lax`, and `Secure` when `TRACE_PUBLIC_URL` is https. Writes must be JSON, which blocks cross-site request forgery.
  - Authenticator codes work once.
  - After 10 failed sign-ins, the account is locked for 15 minutes.
  - Team changes, security settings, tokens and config pushes are recorded in an activity log on the Team page.

## Websites

Switch websites with the menu next to the logo. **Add website** (owners) creates one with its own write key, and its Setup page has snippets ready to paste. Removing a website deletes its data and asks you to type its name.

Everything that works on data takes a website:

- **CLI:** `--site stride` (or `TRACE_SITE`) on `analyze`, `insights`, `prompt`, `status`, `push` and `serve --config`. With a single website it can be left out; with several, the command lists them. `evaluate` runs every website unless you name one. `trace site list|add|remove` manages them from the server.
- **HTTP API:** `/api/sites/{site}/…`, e.g. `GET /api/sites/stride/insights`, or send the `X-Trace-Site` header. The old `/api/…` paths still work for an account with one website (answered with a `Deprecation` header) and return `400 site_required` once there are several.
- **MCP:** the `site` argument, above.

## Plans and limits

Every account is on a plan, and each limit can be overridden for one account:

| Limit | |
|---|---|
| Data kept (days) | Raw events older than this are deleted. The server's `--retention-days` is a cap on top |
| Websites | Per account |
| Events per month | Over the limit, the collector is still answered normally (the host site never notices) but events aren't recorded. The dashboard shows a banner and the count |
| Members | People plus pending invites |
| Claude analyses per month | Analysis with the built-in heuristics is never limited. Past the limit, `auto` falls back to heuristics with a warning, and an explicit `--engine claude` is refused. Runs where Claude wasn't used don't count |

A self-hosted server puts every account on **unlimited**, which has no limits, so you never need to think about plans. The **Plan** page shows an account its limits and this month's usage, never the plan's name; `--contact-url` (or `TRACE_CONTACT_URL`) adds a link for asking for more.

### Operators

Operators run the server: they manage every account's plan, and nothing else. Operator rights are granted only on the server, never from the web:

```sh
trace operator grant you@example.com   # also: revoke, list
```

An operator signs in normally, and the **Admin** area appears once they have turned on two-step verification. It lists every account with its plan, people, websites and this month's usage; edits plan templates (empty means no limit) and the default plan for new accounts; changes an account's plan or overrides single limits ("same as plan", "no limit" or a number); and creates accounts with a single-use invite link for their owner. It never shows an account's analytics, insights or privacy rules. Operator actions are recorded in the operator's own account's activity log. Set the plan new accounts get with `TRACE_DEFAULT_PLAN` (e.g. `free`) or from the Admin area.

Built-in templates: `unlimited` (no limits, the default), `free` (30 days, 1 website, 100,000 events, 3 members, 5 Claude analyses), `friend` (365 days, 5 websites, 1,000,000 events, 10 members, 50 Claude analyses) and `owner` (no limits).

**Upgrading** a database from before accounts turns each website into its own account (people, invites, tokens and the log move with it), keeps a backup at `<db>.pre-accounts`, and turns members and viewers into users. It stops, listing them, if one email address is in more than one workspace.

## Privacy and sensitive data

Trace is built to never collect passwords, payment details, tokens or other secrets, rather than to protect them after the fact. There's no session replay and no DOM snapshot, and nothing users type is ever read. Everything else goes through two layers of scrubbing: the collector in the browser, then the server again at ingest, so old or hand-written clients are held to the same rules.

| Data | What's kept |
|---|---|
| Input values, `contenteditable` content, page titles | Never collected |
| Element text | Only from buttons, links and labels, never from containers. Nothing inside `data-trace-mask` or inside a form with password or payment fields. Every digit is masked: `Pay €1,234.56` → `Pay €#,###.##` |
| Emails, tokens/JWTs/API keys, card numbers, IBANs, phone-like numbers | Replaced with `[email]`, `[token]`, `[card]`, `[iban]`, `[number]` in everything sent |
| URLs | Query strings are never sent. Path segments that look like emails, ids or tokens become `:email`, `:id`, `:token` |
| Referrer | Origin only, and only from another site |
| Custom-event props | Keys such as `password`, `token`, `card`, `cvv`, `iban`, `email`, `phone`, `address`, `dob`, `balance`, `salary` are dropped. Other string values are scrubbed as above |
| User ids from `identify()` | Must be opaque. Email-like ids are hashed (SHA-256) in the browser, and the server drops anything still personal |
| Raw events | Deleted after 90 days (`--retention-days`). Insights, KPIs and evaluations are aggregates and are kept |

The analyst only ever sees this scrubbed, aggregated digest, never individual sessions.

**Your own rules.** On the dashboard's **Privacy** page, admins can add rules on top of the built-in protections:

| Rule | Example | Applied |
|---|---|---|
| Never read text inside | `.account-summary`, `[data-customer]` | in the browser |
| Redact a pattern | `ORD-\d+` → `[order]` | in the browser and again on the server |
| Ignore pages | `/admin/*`, `/account/billing` | nothing at all is recorded there, enforced in both |
| Drop a property | `customer_ref` | in both |

The page tests rules against sample text before you rely on them. Every change goes into the activity log. The server applies changes immediately, and writes the rules into the collector it serves, so browsers pick them up within the hour as their cached copy refreshes. Workspace patterns run before the built-in ones, so `ORD-\d+` still matches before digits are masked.

**What you should still do:** pattern matching can't recognise names or free-text personal details in UI copy, such as a button reading "Message Jane". Add `data-trace-mask` to areas that show customer data (account pages, admin tools, order details), and pass internal ids to `identify()`, not emails. Serve both your site and Trace over HTTPS, and set `TRACE_ADMIN_TOKEN` on any server others can reach.

## Performance

The collector is 6.5 KB on the wire with brotli (7.3 KB with gzip), and is built not to affect the host page. The readable, commented source is in the repo; the server strips comments and compresses it once at startup. It is cached for an hour and then revalidated with an ETag, so collector upgrades reach every site without changing the snippet. Handlers are passive and only note what happened; describing elements and queueing run in `requestIdleCallback`. Nothing reads layout on the hot path (no `innerText`; scroll height is sampled at most every 250 ms). DOM mutations are observed only during the second after a click, to spot dead clicks. Storage writes are throttled, data is sent in batches with `sendBeacon`, and every callback is wrapped so collector errors never reach the page. `data-sample="0.25"` records only a share of visitors; the rest skip setup entirely.

## Install the collector

**Website** (anything where links reload the page):

```html
<script src="https://trace.example.com/d.js?k=WRITE_KEY" data-key="WRITE_KEY" defer></script>
```

**Single-page app** (React, Vue, Svelte, Angular…). History API, Navigation API and hash-router (`#/settings`) navigations are tracked automatically:

```html
<script src="https://trace.example.com/d.js?k=WRITE_KEY" data-key="WRITE_KEY" data-routes="/products/:slug, /docs/*" defer></script>
```

**Tag manager, `type="module"` or bundler**, where the tag can't carry attributes:

```html
<script>window.trace=window.trace||{q:[]};["track","identify","page","init"].forEach(function(m){trace[m]=trace[m]||function(){trace.q.push([m,arguments])}})</script>
<script>trace.init({ key: "WRITE_KEY", routes: ["/products/:slug"] });</script>
<script src="https://trace.example.com/d.js?k=WRITE_KEY" async></script>
```

The one-line stub queues calls made before the script loads, so `trace.track()` is safe to call at any time. Snippets with your write key filled in are on the dashboard's Setup page. The `?k=` in the script URL lets the server send each website a collector with that website's privacy rules built in.

| Option (`data-*` or `init({...})`) | |
|---|---|
| `key` | Write key (required) |
| `endpoint` | Collect URL. Defaults to `/v1/collect` next to `d.js` |
| `routes` | Patterns that group dynamic URLs: `:param` matches one segment, a trailing `*` matches the rest. Numeric, UUID and long-hash segments become `:id` even without a pattern. |
| `sample` | Share of visitors to record, e.g. `0.25` |
| `pageviews` | `auto` (default) or `manual`, in which case you call `trace.page()` yourself |
| `console` | `false` stops recording `console.error` calls. The collector wraps `console.error`, so devtools shows `d.js` as the call site for those messages |

API: `trace.track(name, props)`, `trace.identify(userId)`, `trace.page(route?)`, `trace.flush()`.

Multi-page sites keep one session across page loads (30 minutes of inactivity starts a new one). Pages restored from the back/forward cache count as pageviews, and form submits are sent before the browser navigates away. Query strings are never recorded. The collector sends JSON as `text/plain` through `sendBeacon`, so browsers skip the CORS preflight.

## Configuration

| Variable | Default | |
|---|---|---|
| `TRACE_DB` | `trace.db` | SQLite file, or a Postgres URL (see [Install on a server](#install-on-a-server)) |
| `TRACE_ENV_FILE` | `trace.env` | Settings file every command reads; written by `trace init` |
| `TRACE_ADDR` | `127.0.0.1:4318` | Listen address |
| `TRACE_SITE` | – | Website for commands that work on one (its short name) |
| `TRACE_CONFIG` | – | `trace.json` for `serve` to apply and watch (needs `--site`) |
| `TRACE_ALLOW_SIGNUP` | off | Let anyone create an account from the sign-in page |
| `TRACE_CONTACT_URL` | – | Where the Plan page sends people who need higher limits |
| `TRACE_DEFAULT_PLAN` | `unlimited` | Plan for new accounts, applied when the server starts |
| `TRACE_SERVER` | – | Server URL for `trace push` |
| `TRACE_RETENTION_DAYS` | `90` | Delete raw events older than this (0 keeps them). Caps each plan's own limit |
| `TRACE_TOKEN` | – | Personal API token for `trace push` (create one under Account) |
| `TRACE_ADMIN_TOKEN` | – | Deprecated shared token with admin rights. Prefer personal tokens, which carry their owner's role and name |
| `ANTHROPIC_API_KEY` | – | Turns on the Claude analyst |
| `TRACE_MODEL` | `claude-opus-5-5` | Analyst model |
| `TRACE_EFFORT` | `high` | `output_config.effort` for the analyst |
| `ANTHROPIC_BASE_URL` | `https://api.anthropic.com` | |

On the first-party API, the analyst opts into server-side refusal fallbacks (`fallbacks: "default"`), so a request the main model declines is retried on Anthropic's recommended fallback model.

## CLI

```
trace [--site S] ...
           init [--database sqlite|postgres] [--db TARGET] [--force]   # choose the database, save it to trace.env
           serve [--config trace.json] [--demo-site ADDR] [--allow-signup] [--contact-url URL]
           check [FILE] | push [FILE] [--server URL --token T]
           analyze [--engine auto|claude|heuristics] [--days 14] [--max-insights 5] | evaluate
           insights [--status new] | prompt <id> | status <id> <status> [--reason ...] | demo
           site list | site add NAME [--account A] | site remove SLUG --confirm NAME
           user create --email E [--role owner|admin|user] [--account A] | user list
           user reset-password --email E | user reset-mfa --email E
           operator grant EMAIL | operator revoke EMAIL | operator list
           mcp [--account A]    # MCP server on stdio for a local agent; remote agents use POST /mcp
```

## HTTP API

`GET /d.js?k=WRITE_KEY` (collector) · `POST /v1/collect` (write key) · `POST /mcp` (MCP, API token) · with an API token: `GET /api/sites` · `GET /api/plan` · and per website, under `/api/sites/{site}`: `GET|POST /config` · `GET /kpis` · `GET /digest?days=14` · `GET /insights?status=` · `GET /insights/{id}/prompt` · `POST /insights/{id}/status` · `POST /insights/{id}/kpi` · `POST /analyze` · `POST /evaluate` · `GET /runs`

## Contributing

Tests run on SQLite by default; `TRACE_TEST_POSTGRES=postgres://localhost/postgres cargo test` runs the same suite on Postgres, giving each test its own throwaway database on that server.

Contributions come in as OpenSpec changes filed as GitHub issues. Explore the problem with `/opsx:explore`, draft the change with `/opsx:propose`, then run `scripts/openspec-issue.sh <change-id>` to validate it and open the issue. An automated check validates every proposal, and maintainers review it for privacy and site impact. See [CONTRIBUTING.md](CONTRIBUTING.md), and report security problems privately ([SECURITY.md](SECURITY.md)).

## Layout

- `crates/trace-core`: storage, ingestion, KPI measurement, digest, insight lifecycle, prompt rendering, evaluation
- `crates/trace-agent`: analyst (Claude client and heuristics)
- `site/`: the marketing site (static, published to GitHub Pages by `.github/workflows/pages.yml`)
- `deploy/` and `Dockerfile`: how the hosted server runs (Docker behind a shared Caddy, on a shared Postgres); see [deploy/README.md](deploy/README.md)
- `crates/trace-server`: the `trace` binary (HTTP server, CLI, demo seeder, embedded dashboard and collector)

## License

Trace is licensed under the [GNU Affero General Public License v3.0](LICENSE). You can run, study, change and share it. If you offer a modified version to others over a network, you must make your changes available under the same licence. Running it unmodified, or for yourself, asks nothing of you.

The collector, `crates/trace-server/assets/d.js`, is the one file that runs inside your website, and it is licensed under the [MIT License](crates/trace-server/assets/LICENSE-collector) instead, so adding the script tag to a site carries no copyleft obligations.
