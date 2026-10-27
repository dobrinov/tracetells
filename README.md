# Diagnostic

**[diagnostic.dev](https://diagnostic.dev)**: behaviour analytics built for agents.

Diagnostic collects how people use your product, an analyst agent turns that into specific improvement ideas tied to your KPIs, you copy an idea as a prompt for your coding agent, and once it ships Diagnostic measures whether it worked.

```
collect ──▶ digest ──▶ analyst ──▶ ideas ──▶ prompt ──▶ ship ──▶ evaluate
 (d.js)    (SQLite)   (Claude or   new → selected → in progress → released
                       heuristics)                      ╰── dismissed (with a reason the analyst learns from)
```

## Try it in a minute

```sh
cargo build --release
./target/release/diagnostic --db demo.db demo       # 30 days of simulated traffic for "Stride", a demo shoe shop
./target/release/diagnostic --db demo.db analyze    # create ideas
./target/release/diagnostic --db demo.db serve --demo-site 127.0.0.1:4319 \
    --config crates/diagnostic-server/assets/shop/diagnostic.json
```

Open the dashboard at http://127.0.0.1:4318 and sign in as `owner@example.com`, `admin@example.com`, `member@example.com` or `viewer@example.com`, all with the password `1`, to see each role. (Demo accounts only: real accounts need at least 10 characters.) Then open the Stride shop, instrumented with the collector, at http://127.0.0.1:4319. The shop has four planted problems: product images that look clickable but aren't, a "Pay now" button that takes 3 seconds with no feedback, a pricing toggle that throws, and a signup form that wipes itself on a bad phone number. Your clicks show up in the dashboard next to the simulated history.

## Pieces

| | |
|---|---|
| **Collector** (`/d.js`) | One script tag. Captures pageviews (single-page apps and full reloads), clicks, rage clicks, dead clicks, errors, scroll depth, time on page, form starts and submits, plus custom events from `diagnostic.track(name, props)` or `data-diagnostic-event="name"` markup. It never records input values. |
| **KPIs as code** | KPIs and the custom-event catalogue live in `diagnostic.json` in your repository, next to the code that sends the events. `diagnostic check` validates it, `diagnostic push` publishes it (from CI, say), and the dashboard shows results and tracking health, but never edits definitions. |
| **Analyst** | Builds an aggregated digest and asks Claude for ideas through structured output. It sends your KPIs plus existing and dismissed ideas so it doesn't repeat itself. Without an API key, or when Claude fails, built-in heuristics take over. |
| **Prompts** | Each idea is rendered as a self-contained task for a coding agent: problem, evidence, goal KPI, proposal, routes and acceptance criteria. Copy it from the dashboard, or run `diagnostic prompt 12 \| claude`. |
| **Errors** | Five kinds: uncaught exceptions, unhandled promise rejections, `console.error` calls (where React, Vue and Angular report the errors they catch), failed `fetch`/XHR requests (status ≥ 400 or no response) and scripts, stylesheets or images that fail to load. |
| **Evaluation** | For released ideas, compares the KPI in equal windows before and after the release (Welch test, at least 30 sessions per side) and gives a verdict: improved, regressed, no change or insufficient data. It runs every hour while the server is up. |

## KPIs and events in code

Put a `diagnostic.json` at the root of your application's repository:

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

Send the events from code with `diagnostic.track("purchase", { plan })`, or with no JavaScript at all: `<button data-diagnostic-event="add_to_cart" data-diagnostic-props='{"sku":"S-1"}'>`. On a `<form>`, the event fires when it's submitted.

Then validate and publish:

```sh
diagnostic check                     # validate; non-zero exit on errors, so it can gate CI
diagnostic push --server https://diagnostic.example.com --token "$DIAGNOSTIC_ADMIN_TOKEN"
```

`check` reports every problem with its location, e.g. `kpis.checkout.conversion.from: unknown event type "pagview"`. `push` records the git commit it came from. KPIs are matched by key, so editing one keeps its history and the ideas linked to it. A KPI removed from the file is archived; released ideas that targeted it are still evaluated. When self-hosting, `diagnostic serve --config diagnostic.json` applies the file at startup and again whenever it changes.

## MCP: use Diagnostic from your coding agent

Diagnostic is also an MCP server, so an agent such as Claude Code or Cursor can run the whole loop: read the KPIs, pick an idea, fetch it as a task prompt, implement it and mark it released, after which Diagnostic measures the impact. Like the dashboard, it only exposes aggregated patterns, never individual sessions.

| Tool | |
|---|---|
| `get_overview` | Collection status, KPIs vs targets, ideas per status |
| `get_behaviour_summary` | The aggregated digest the analyst reads |
| `list_kpis`, `get_tracking_health` | KPI values; whether declared events are arriving |
| `list_ideas`, `get_idea`, `get_idea_prompt` | Find work, and get it as a complete implementation task |
| `update_idea_status` | Select, start, release or dismiss an idea (member) |
| `run_analysis`, `evaluate_releases` | Create ideas; measure released ones (member) |
| `check_config` | Validate a `diagnostic.json` being edited |
| `get_privacy_rules` | What's protected (read-only) |

Agents can't change privacy rules or people, which stay in the dashboard, or KPIs, which stay in `diagnostic.json`. Status changes and analysis runs appear in the activity log under the token's name.

**Remote** (any server; Streamable HTTP, authenticated with a personal API token from **Account**):

```sh
claude mcp add --transport http diagnostic https://diagnostic.example.com/mcp \
    --header "Authorization: Bearer dgt_..."
```

Other clients: `{"mcpServers": {"diagnostic": {"url": "https://diagnostic.example.com/mcp", "headers": {"Authorization": "Bearer dgt_..."}}}}`

**Local** (on the machine with the database; stdio):

```sh
claude mcp add diagnostic -- diagnostic --db /path/to/diagnostic.db mcp
```

## Accounts and access

Works for one person or a whole company.

- **Getting started alone:** the first time `diagnostic serve` starts, it prints a one-time setup link. Open it to create the owner account. (Or run `diagnostic user create --email you@example.com` on the server.) The link stops working once the owner exists, so whoever reaches a fresh server first can't claim it.
- **Teams:** admins invite people from the **Team** page by role and share the invite link themselves. Diagnostic sends no email. Each link works once and expires in 7 days.

  | Role | Can |
  |---|---|
  | Owner | Everything (one per workspace, can't be removed) |
  | Admin | Manage the team, two-step policy and privacy rules; see and revoke anyone's API tokens |
  | Member | Move ideas through their statuses, link KPIs, run analysis |
  | Viewer | Look, but not change anything |

- **Two-step verification (TOTP):** anyone can add an authenticator app (1Password, Google Authenticator, Authy…) under **Account**. Setting it up gives 10 single-use recovery codes. Admins can **require it for everyone**: members without it are held at a setup screen the next time they sign in, and an admin can only turn the requirement on after enabling it for their own account. If someone loses their phone, an admin can reset their two-step verification, and a locked-out owner can use `diagnostic user reset-mfa --email …` or `diagnostic user reset-password --email …` on the server.
- **API tokens are personal.** Everyone creates their own under **Account**, for MCP agents and CI.
  - A token acts with its owner's *current* role, or only reads if it's made read-only. Demoting someone demotes their tokens at once, and removing them revokes their tokens.
  - Tokens can't manage people, tokens or accounts.
  - The activity log shows who acted, e.g. `ana@acme.example via token "Claude Code"`.
  - Admins see and revoke everyone's tokens on **Team**.
  - Pass a token to `diagnostic push --token` (or `DIAGNOSTIC_TOKEN`).
  - The old shared `DIAGNOSTIC_ADMIN_TOKEN` still works but is deprecated, because it belongs to nobody.
- **How it's protected:**
  - Passwords are hashed with Argon2id. Sessions, invite links, API tokens and recovery codes are stored only as SHA-256 hashes, so a copy of the database can't be used to sign in.
  - Session cookies are `HttpOnly`, `SameSite=Lax`, and `Secure` when `DIAGNOSTIC_PUBLIC_URL` is https. Writes must be JSON, which blocks cross-site request forgery.
  - Authenticator codes work once.
  - After 10 failed sign-ins, the account is locked for 15 minutes.
  - Team changes, security settings, tokens and config pushes are recorded in an activity log on the Team page.

## Privacy and sensitive data

Diagnostic is built to never collect passwords, payment details, tokens or other secrets, rather than to protect them after the fact. There's no session replay and no DOM snapshot, and nothing users type is ever read. Everything else goes through two layers of scrubbing: the collector in the browser, then the server again at ingest, so old or hand-written clients are held to the same rules.

| Data | What's kept |
|---|---|
| Input values, `contenteditable` content, page titles | Never collected |
| Element text | Only from buttons, links and labels, never from containers. Nothing inside `data-diagnostic-mask` or inside a form with password or payment fields. Every digit is masked: `Pay €1,234.56` → `Pay €#,###.##` |
| Emails, tokens/JWTs/API keys, card numbers, IBANs, phone-like numbers | Replaced with `[email]`, `[token]`, `[card]`, `[iban]`, `[number]` in everything sent |
| URLs | Query strings are never sent. Path segments that look like emails, ids or tokens become `:email`, `:id`, `:token` |
| Referrer | Origin only, and only from another site |
| Custom-event props | Keys such as `password`, `token`, `card`, `cvv`, `iban`, `email`, `phone`, `address`, `dob`, `balance`, `salary` are dropped. Other string values are scrubbed as above |
| User ids from `identify()` | Must be opaque. Email-like ids are hashed (SHA-256) in the browser, and the server drops anything still personal |
| Raw events | Deleted after 90 days (`--retention-days`). Ideas, KPIs and evaluations are aggregates and are kept |

The analyst only ever sees this scrubbed, aggregated digest, never individual sessions.

**Your own rules.** On the dashboard's **Privacy** page, admins can add rules on top of the built-in protections:

| Rule | Example | Applied |
|---|---|---|
| Never read text inside | `.account-summary`, `[data-customer]` | in the browser |
| Redact a pattern | `ORD-\d+` → `[order]` | in the browser and again on the server |
| Ignore pages | `/admin/*`, `/account/billing` | nothing at all is recorded there, enforced in both |
| Drop a property | `customer_ref` | in both |

The page tests rules against sample text before you rely on them. Every change goes into the activity log. The server applies changes immediately, and writes the rules into the collector it serves, so browsers pick them up within the hour as their cached copy refreshes. Workspace patterns run before the built-in ones, so `ORD-\d+` still matches before digits are masked.

**What you should still do:** pattern matching can't recognise names or free-text personal details in UI copy, such as a button reading "Message Jane". Add `data-diagnostic-mask` to areas that show customer data (account pages, admin tools, order details), and pass internal ids to `identify()`, not emails. Serve both your site and Diagnostic over HTTPS, and set `DIAGNOSTIC_ADMIN_TOKEN` on any server others can reach.

## Performance

The collector is 6.5 KB on the wire with brotli (7.3 KB with gzip), and is built not to affect the host page. The readable, commented source is in the repo; the server strips comments and compresses it once at startup. It is cached for an hour and then revalidated with an ETag, so collector upgrades reach every site without changing the snippet. Handlers are passive and only note what happened; describing elements and queueing run in `requestIdleCallback`. Nothing reads layout on the hot path (no `innerText`; scroll height is sampled at most every 250 ms). DOM mutations are observed only during the second after a click, to spot dead clicks. Storage writes are throttled, data is sent in batches with `sendBeacon`, and every callback is wrapped so collector errors never reach the page. `data-sample="0.25"` records only a share of visitors; the rest skip setup entirely.

## Install the collector

**Website** (anything where links reload the page):

```html
<script src="https://diagnostic.example.com/d.js" data-key="WRITE_KEY" defer></script>
```

**Single-page app** (React, Vue, Svelte, Angular…). History API, Navigation API and hash-router (`#/settings`) navigations are tracked automatically:

```html
<script src="https://diagnostic.example.com/d.js" data-key="WRITE_KEY" data-routes="/products/:slug, /docs/*" defer></script>
```

**Tag manager, `type="module"` or bundler**, where the tag can't carry attributes:

```html
<script>window.diagnostic=window.diagnostic||{q:[]};["track","identify","page","init"].forEach(function(m){diagnostic[m]=diagnostic[m]||function(){diagnostic.q.push([m,arguments])}})</script>
<script>diagnostic.init({ key: "WRITE_KEY", routes: ["/products/:slug"] });</script>
<script src="https://diagnostic.example.com/d.js" async></script>
```

The one-line stub queues calls made before the script loads, so `diagnostic.track()` is safe to call at any time. Snippets with your write key filled in are on the dashboard's Setup page.

| Option (`data-*` or `init({...})`) | |
|---|---|
| `key` | Write key (required) |
| `endpoint` | Collect URL. Defaults to `/v1/collect` next to `d.js` |
| `routes` | Patterns that group dynamic URLs: `:param` matches one segment, a trailing `*` matches the rest. Numeric, UUID and long-hash segments become `:id` even without a pattern. |
| `sample` | Share of visitors to record, e.g. `0.25` |
| `pageviews` | `auto` (default) or `manual`, in which case you call `diagnostic.page()` yourself |
| `console` | `false` stops recording `console.error` calls. The collector wraps `console.error`, so devtools shows `d.js` as the call site for those messages |

API: `diagnostic.track(name, props)`, `diagnostic.identify(userId)`, `diagnostic.page(route?)`, `diagnostic.flush()`.

Multi-page sites keep one session across page loads (30 minutes of inactivity starts a new one). Pages restored from the back/forward cache count as pageviews, and form submits are sent before the browser navigates away. Query strings are never recorded. The collector sends JSON as `text/plain` through `sendBeacon`, so browsers skip the CORS preflight.

## Configuration

| Variable | Default | |
|---|---|---|
| `DIAGNOSTIC_DB` | `diagnostic.db` | SQLite file |
| `DIAGNOSTIC_ADDR` | `127.0.0.1:4318` | Listen address |
| `DIAGNOSTIC_CONFIG` | – | `diagnostic.json` for `serve` to apply and watch |
| `DIAGNOSTIC_SERVER` | – | Server URL for `diagnostic push` |
| `DIAGNOSTIC_RETENTION_DAYS` | `90` | Delete raw events older than this (0 keeps them) |
| `DIAGNOSTIC_TOKEN` | – | Personal API token for `diagnostic push` (create one under Account) |
| `DIAGNOSTIC_ADMIN_TOKEN` | – | Deprecated shared token with admin rights. Prefer personal tokens, which carry their owner's role and name |
| `ANTHROPIC_API_KEY` | – | Turns on the Claude analyst |
| `DIAGNOSTIC_MODEL` | `claude-opus-5-5` | Analyst model |
| `DIAGNOSTIC_EFFORT` | `high` | `output_config.effort` for the analyst |
| `ANTHROPIC_BASE_URL` | `https://api.anthropic.com` | |

On the first-party API, the analyst opts into server-side refusal fallbacks (`fallbacks: "default"`), so a request the main model declines is retried on Anthropic's recommended fallback model.

## CLI

```
diagnostic serve [--config diagnostic.json] [--demo-site ADDR] | check [FILE] | push [FILE] [--server URL --token T]
           analyze [--engine auto|claude|heuristics] [--days 14] [--max-ideas 5] | evaluate
           ideas [--status new] | prompt <id> | status <id> <status> [--reason ...] | demo
           user create --email E [--role R] | user list | user reset-password --email E | user reset-mfa --email E
           mcp                  # MCP server on stdio for a local agent; remote agents use POST /mcp
```

## HTTP API

`POST /v1/collect` (collector, write key) · `POST /mcp` (MCP, API token) · admin token: `GET|POST /api/config` · `GET /api/kpis` · `GET /api/digest?days=14` · `GET /api/ideas?status=` · `GET /api/ideas/{id}/prompt` · `POST /api/ideas/{id}/status` · `POST /api/ideas/{id}/kpi` · `POST /api/analyze` · `POST /api/evaluate` · `GET /api/runs`

## Layout

- `crates/diagnostic-core`: storage, ingestion, KPI measurement, digest, idea lifecycle, prompt rendering, evaluation
- `crates/diagnostic-agent`: analyst (Claude client and heuristics)
- `crates/diagnostic-server`: the `diagnostic` binary (HTTP server, CLI, demo seeder, embedded dashboard and collector)
