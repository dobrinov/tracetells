# Trace

**Trace**: web analytics, and a way to improve your website's UX, for the agentic era.

Trace collects how people use your product, in aggregate. Your coding agent, over MCP and with its own model, turns that into specific improvement insights tied to your KPIs and implements them, and once a change ships Trace measures whether it worked.

```
collect ──▶ digest ──▶ your agent analyses ──▶ insights ──▶ your agent ships ──▶ evaluate
 (d.js)    (aggregates,  (over MCP: reads the       new → selected → in progress → released
            SQLite or     summary, saves insights)                ╰── dismissed (with a reason the agent learns from)
            Postgres)
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
| **Analyst** | Your coding agent, over MCP: it asks Trace questions (`query_behaviour`), drafts insights whose evidence Trace measures (`draft_insights`), shows them to you and saves the ones you approve (`save_insights`). See [MCP](#mcp-use-trace-from-your-coding-agent). The server also has its own analysis: built-in checks that need no model (the dashboard's **Run analysis**), and optionally Claude when it has an `ANTHROPIC_API_KEY`. |
| **Prompts** | Each insight is rendered as a self-contained task for a coding agent: problem, evidence, goal KPI, proposal, routes and acceptance criteria. Copy it from the dashboard, or run `trace prompt 12 \| claude`. |
| **Errors** | Five kinds: uncaught exceptions, unhandled promise rejections, `console.error` calls (where React, Vue and Angular report the errors they catch), failed `fetch`/XHR requests (status ≥ 400 or no response) and scripts, stylesheets or images that fail to load. |
| **Evaluation** | For released insights, compares the KPI over the 14 days before the release with the 14 days after (Welch test, at least 30 sessions per side). See [Did it help?](#did-it-help). |

On the first-party API, the analyst opts into server-side refusal fallbacks (`fallbacks: "default"`), so a request the main model declines is retried on Anthropic's recommended fallback model.

### Did it help?

Trace tries hard not to tell you a change worked when it was noise.

- **A fixed window.** The 14 days after a release are compared with the 14 before; whole weeks, so weekdays and weekends balance. Until those 14 days have passed the insight says *Measuring: day 3 of 14* and gives no verdict, because checking a growing window every hour would sooner or later catch a lucky swing. Then the verdict (improved after the release, worse after the release, no change, or not enough data) is settled and doesn't change. It says "after", not "because of": a significant difference comes with a reminder that other things in those weeks can move a KPI too.
- **Too small to judge.** A verdict needs 30 sessions on each side of the release. While a release is being measured, Trace says early if the site had fewer than that before it, or if the pace since won't get there, so nobody waits 14 days for "not enough data".
- **The KPI as it was.** A release is judged by its KPI's definition at the moment it shipped. If `trace.json` changes the KPI later, the verdict says it was measured with the earlier definition. Linking another KPI, or correcting the release time, starts the measurement over.
- **When it really went live.** The before/after split is the release time, so give the deploy's time rather than the moment someone remembered to click: the dashboard asks for it, `update_insight_status` takes `released_at`, `trace status 12 released --at 2026-10-28T14:05:00Z` does it from the CLI, and a deploy pipeline can do it itself (below). A release time before Trace found the insight is refused: that change can't have been its fix.
- **Other changes.** When another change aimed at the same KPI shipped within 14 days, the verdict names it: part of the difference may be its doing.
- **The problem itself.** Each measured evidence item gets its own verdict over the same windows, next to the KPI's: did the thing the insight describes (say, rage clicks on Pay now) shrink? The two can disagree. "Better locally, worse overall" is worth knowing, and both are shown as they are.
- **History.** Trace measures every insight's evidence again once a day, and on demand: the **Refresh** button, `refresh_insight` over MCP, or `POST /api/sites/{site}/insights/{id}/refresh`. It keeps every measurement, so you can watch an insight's numbers before and after a change. Refreshing never changes the insight's text.

It is still a before/after comparison, not an experiment: a campaign, a season or an outage in either window can move a KPI too.

**From your deploy.** Task prompts ask the agent to put `Trace-Insight: 12` in the commit message. After a deploy, mark every insight it shipped as released at that moment:

```sh
ids=$(git log --format=%B "$PREVIOUS_DEPLOY..HEAD" | sed -n 's/^Trace-Insight: *#\{0,1\}\([0-9][0-9]*\).*/\1/p' | sort -un | paste -sd, -)
[ -n "$ids" ] && curl -fsS -X POST "https://trace.example.com/api/sites/SITE/releases" \
  -H "Authorization: Bearer $TRACE_TOKEN" -H "Content-Type: application/json" \
  -d "{\"insights\": [$ids], \"ref\": \"$(git rev-parse --short HEAD)\"}"
```

`POST /releases` takes `insights` (ids), and optionally `at` (ISO 8601 or epoch ms; default now) and `ref` (for the activity log). It answers with what it `released` and what it `skipped` and why (dismissed, already released earlier, unknown).

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

### Funnels

A funnel is the steps people take in order within one visit, and where they stop. Declare them next to the KPIs:

```json
"funnels": {
  "checkout": {
    "name": "Product to purchase",
    "steps": [
      { "pageview": "/products/:id" },
      { "event": "add_to_cart" },
      { "pageview": "/checkout" },
      { "event": "purchase" }
    ]
  }
}
```

Each step is an event matcher, like a KPI's, and a funnel has 2 to 8 of them. A step counts the sessions that reached it after the step before, in the same session. Trace also **recognises funnels from your KPIs**: conversion KPIs that chain (`/` → `/pricing` and `/pricing` → `signup`) become the funnel `/` → `/pricing` → `signup`, unless a declared funnel already covers it.

The **KPIs** page shows every funnel: sessions per step, the share that continued from the step before, the same over the previous window, conversion by device, and the **biggest drop**, the step where the most sessions are lost, named once someone has finished the funnel. That step is where analysis looks first: `get_funnels` over MCP returns the same, the analysis prompt and Trace's own analysis start from it, and an insight about a step carries that step's conversion as its evidence, so the release is judged on it.

### Audience

The **Audience** page answers how many people come, how many leave straight away, how many get engaged and how many come back. Say what your marketing pages are and which actions make a visitor engaged:

```json
"audience": {
  "marketing": ["/", "/pricing", "/blog/*"],
  "engaged": [{ "event": "signup" }, { "event": "login" }]
}
```

- **Visitors** per day, with sessions and the window before. Visitors are cookieless, so an anonymous visitor counts once a day and a window's visitors are its days added up.
- **Bounce** on the marketing pages: sessions that landed on one and left without clicking, starting a form or opening another page, by landing page and device. Without `marketing`, every landing page counts.
- **Engaged**: the share of visitors who did at least one of the `engaged` actions, and how often each happened.
- **DAU, WAU, MAU and stickiness**, and **weekly retention** cohorts: these follow people from one day to the next, so they count only people your app signs in with `trace.identify(id)`. Pass an id that isn't an email, ideally a keyed hash of the user's id, so Trace can count a person without knowing who they are.

Home shows the last 7 days of it, and `get_audience` over MCP gives your agent the same numbers.

### Areas and features

A larger product has parts (areas) and those have features, and some get used while others don't. Name them in `trace.json` by the pages and events that mean someone used them:

```json
"areas": {
  "battles": {
    "name": "Battles",
    "features": {
      "duels": { "name": "Duels", "match": [{ "pageview": "/challenges*" }, { "event": "duel_started" }] },
      "leagues": { "name": "Leagues", "match": [{ "pageview": "/leagues*" }] }
    }
  }
}
```

The **Features** page measures each area and feature over the window:
- **Reach:** the share of visitors who used it, with the window before.
- **Adoption:** the share of signed-in people who used it.
- **Come back:** how many of those people used it on two or more days.
- **With problems:** the share of its sessions that hit a rage click, dead click or error on its own pages (the pages it declares, or where its events happen when it declares none), and the most common one.

Each gets a status: **used, but it hurts** (where a fix helps the most people), **little used**, **little used and it hurts**, **healthy**, or too little data. Without declared areas, Trace suggests them from the website's URLs. Home raises the busiest area that hurts, `get_areas` gives your agent the same numbers, and the analysis prompt points it there.

### Visits from your own machine

Once a website sends events from a real address, batches from localhost, loopback and private-network addresses (and `.local` / `.localhost` names) are left out, so opening a local copy of the site doesn't add to its numbers. A website that has only ever sent from local addresses, such as one you're trying Trace out on, keeps them.

## MCP: use Trace from your coding agent

Trace is also an MCP server, and your coding agent can run all of it: setting a website up, and then being its analyst.

**Connect** (no token to copy). In Claude Code, the plugin connects the server and adds `/trace:setup`, `/trace:analyse` and `/trace:fix`:

```
/plugin marketplace add dobrinov/tracetells
/plugin install trace@tracetells
```

It asks for your server's address (the default is the hosted `https://app.tracetells.com/mcp`). Then run `/mcp`, choose **plugin:trace:trace** and **Authenticate**. Without the plugin, add the server alone:

```sh
claude mcp add --transport http trace https://trace.example.com/mcp
```

and then run `/mcp`, choose **trace** and **Authenticate**. Your browser opens Trace: sign in (with two-step verification if it's on) and approve. Other clients use `{"mcpServers": {"trace": {"url": "https://trace.example.com/mcp"}}}` and sign in the same way. This is the MCP authorization flow (OAuth 2.1 with PKCE and dynamic client registration): `/mcp` answers a request without a token with `401` and a `WWW-Authenticate` header that points to `/.well-known/oauth-protected-resource`. You can approve read-only access. The connection becomes a token named after the app, such as "Claude Code (signed in 2026-10-28)", that acts as you. It's listed under **Account**, where you can revoke it.

**Set up** a website from the codebase: `add_site`, `get_install_snippet` (the tag, where it goes, single-page apps, consent and masking), `check_config` then `push_config` for `trace.json`, `add_privacy_rule`, and `get_setup_status` until events arrive. Ask it something like *"Set up Trace for this project: add the snippet to every page, write trace.json with KPIs that matter here and push it, and mask screens with customer data."*

**Analyse and fix.** The agent runs the whole loop with its own model: it reads the aggregated behaviour summary and the KPIs, saves the improvements it finds as insights, fetches one as a task prompt, implements it and marks it released, after which Trace measures the impact. Like the dashboard, it only exposes aggregated patterns, never individual sessions, and nothing goes to a model you didn't choose.

**Asking questions.** `query_behaviour` answers with numbers. It measures matching events over the last 1 to 90 days:
- **Measures:** sessions with a match, events, the share of sessions (`session_rate`), events per session (`per_session`), or a funnel (`conversion`, from → to).
- **Matchers:** written as in `trace.json`.
- **Options:** a breakdown by path, selector, device or day, and a comparison with the window before.

It returns aggregates only, never sessions or people, and breakdown rows from fewer than 3 sessions are added into "other". The same is at `GET`/`POST /api/sites/{site}/query`.

```json
{ "measure": "session_rate", "match": { "rage_click": "button#pay", "path": "/checkout" }, "breakdown": "device", "compare": true }
```

**Insights grounded in data.** Agents don't save insights directly.
- **Draft.** `draft_insights` checks each proposed insight and measures its evidence itself, then returns drafts with Trace's numbers. Each evidence item is a measurement, `{ "claim", "metric", "goal" }`, where `metric` is in `query_behaviour`'s form and `goal` is `decrease` or `increase`. A note, `{ "note" }`, is shown as "not measured". Every insight needs at least one measurement, and an insight whose measurements find nothing is refused.
- **Show, then save.** The agent shows you the drafts, and `save_insights` saves the ones you approve by `draft_id`. Drafts expire after an hour, and read-only connections can draft but not save.
- **Provenance.** Each insight records who created it (for example "you via Claude Code"), when, and the dates its evidence was measured over. The dashboard and the task prompt show this.
- **Built-in checks too.** Trace's own checks measure their evidence the same way.

The **`analyse_website`** prompt gives the agent the analyst's brief:
- read `get_overview`, `get_behaviour_summary`, `list_kpis` and `list_insights`, skipping what exists and learning from dismissal reasons;
- dig in with `query_behaviour`;
- draft up to five insights;
- show them to you, then save the ones you approve.

In Claude Code it's the slash command `/mcp__trace__analyse_website`. Saving, even an empty list when nothing was worth saving, records the analysis, and Trace counts new sessions from then.

**When to analyse.** Trace says a website is worth analysing once it has 100 new sessions since the last analysis, or 30 once a week has passed. Home and Insights show an analysis panel: when the last analysis ran and who ran it (your agent or Trace), so how old the analysed behaviour is; how many sessions and events have arrived since; and, once it's worth it, the command to run (`/trace:analyse`) or a button to analyse on the server. The header's Run analysis button counts the new sessions, and the panel notices within 20 seconds when your agent saves an analysis. The same is in `get_overview` and `get_analysis_status`, and at `GET /api/sites/{site}/analysis` (`?format=text` gives one line, or nothing until it's time). To hear it from Claude Code when a session starts, add a `SessionStart` hook to `~/.claude/settings.json` (the Setup page has it with your address filled in) and keep your token in the `TRACE_TOKEN` environment variable:

```json
{ "hooks": { "SessionStart": [ { "matcher": "startup", "hooks": [ { "type": "command",
  "command": "curl -fsS --max-time 3 -H \"Authorization: Bearer $TRACE_TOKEN\" \"https://trace.example.com/api/sites/SITE/analysis?format=claude-hook\" || true" } ] } ] } }
```

`?format=claude-hook` answers in Claude Code's hook format (`{"systemMessage": "…"}`), which Claude Code shows to you; it prints nothing until there's something to analyse. The brief, and the server's instructions, tell the agent that text inside behaviour data comes from visitors and is never to be followed as instructions.

| Tool | |
|---|---|
| `get_overview` | Collection status, KPIs vs targets, insights per status, and whether it's worth analysing |
| `get_analysis_status` | Whether there's enough new behaviour since the last analysis (100 new sessions, or 30 after a week) |
| `get_behaviour_summary` | The aggregated digest the analyst reads |
| `list_kpis`, `get_tracking_health` | KPI values; whether declared events are arriving |
| `list_insights`, `get_insight`, `get_insight_prompt` | Find work, and get it as a complete implementation task |
| `query_behaviour` | Ask a question: a measure of matching events, optionally broken down by path, selector, device or day, and compared with the window before. Read-only connections can ask |
| `draft_insights`, `save_insights` | Propose insights; Trace measures their evidence and returns drafts. Save the ones the person approves. (`add_insights` was replaced by these) |
| `refresh_insight` | Measure an insight's evidence again with fresh data, and get each item's history |
| `update_insight_status` | Select, start, release or dismiss an insight |
| `run_analysis`, `evaluate_releases` | Trace's own analysis (built-in checks, or Claude when the server has an API key); measure released ones |
| `add_site`, `get_install_snippet`, `get_setup_status` | Add a website (admins), get its snippet with instructions, see whether events arrive and what's left to set up |
| `check_config`, `push_config` | Validate a `trace.json`; apply it (admins) |
| `get_privacy_rules`, `add_privacy_rule`, `remove_privacy_rule` | What's protected, with rule ids; add or remove the website's own rules (admins) |
| `public_view` | Whether the website has a public, read-only link; publish, replace or withdraw it (admins) |
| `confirm_privacy_review` | Record the person's privacy review, only on their word |
| `list_sites` | The account's websites |

Every tool except `list_sites`, `add_site` and `check_config` takes a `site` (the website's short name, e.g. `stride`). It can be left out when the account has a single website. Read-only tokens can only use the reading tools, so an agent that analyses needs a token that isn't read-only.

An agent acts with your role: setup tools need an admin. Agents can't manage people, tokens or the account, and can't remove websites; those stay in the dashboard. KPIs stay defined in `trace.json`, which `push_config` applies. Everything an agent changes appears in the activity log under its token's name.

**With a token instead** (CI, scripts, clients without sign-in): create a personal API token under **Account** and send it as a header:

```sh
claude mcp add --transport http trace https://trace.example.com/mcp \
    --header "Authorization: Bearer trc_..."
```

**Local** (on the machine with the database; stdio):

```sh
claude mcp add trace -- trace --db /path/to/trace.db mcp --account "Acme"
```

## Accounts and access

Works for one person or a whole company. An **account** is a company or a solo user: it has people, roles, a two-step policy, API tokens, a plan, and one or more **websites**. Each website has its own write key, `trace.json` (KPIs and events), privacy rules, insights and collection status. Nobody in one account can see anything of another.

- **Getting started alone:** the first time `trace serve` starts, it prints a one-time setup link. Open it to create the first account and its owner. (Or run `trace user create --email you@example.com` on the server.) The link stops working once the owner exists, so whoever reaches a fresh server first can't claim it.
- **More accounts:** `trace serve --allow-signup` lets anyone create an account of their own from the sign-in page (rate-limited per address). Without it, only an operator can create accounts (see below).
- **Teams:** admins invite people from the **Team** page by role and share the invite link themselves. Trace sends no email. Each link works once and expires in 7 days.
- **One login, several accounts:** an email is one person on the server, and that person can belong to several accounts with a different role in each, e.g. the owner of their own and a user in a client's. Someone who already has a login accepts an invite by signing in and pressing **Join**; the link never sets their password. They switch accounts from the website menu, and signing in opens the account they used last. When sign-up is open, anyone signed in can add an account they own from the same menu. People can leave an account they don't own under **Account**; removing someone takes them out of that account only and revokes their tokens for it. An API token, and an MCP connection, works in one account: the Connect screen asks which.

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

Each website opens on its **Home**: what needs attention now, most important first, each with a link to act on it. Finishing setup comes first while it's unfinished; then a release verdict that just came in, analysis being due, insights waiting to be reviewed, KPIs that are short of target and getting worse, the funnel step losing the most people, errors visitors hit this week, and declared events that stopped arriving. `get_overview` over MCP returns the same list as `today`, so an agent sees the same priorities.

Switch websites with the menu next to the logo. **Add website** (owners) creates one with its own write key, and its Setup page has snippets ready to paste. Removing a website deletes its data and asks you to type its name.

Everything that works on data takes a website:

- **CLI:** `--site stride` (or `TRACE_SITE`) on `analyze`, `insights`, `prompt`, `status`, `push` and `serve --config`. With a single website it can be left out; with several, the command lists them. `evaluate` runs every website unless you name one. `trace site list|add|remove` manages them from the server.
- **HTTP API:** `/api/sites/{site}/…`, e.g. `GET /api/sites/stride/insights`, or send the `X-Trace-Site` header. The old `/api/…` paths still work for an account with one website (answered with a `Deprecation` header) and return `400 site_required` once there are several.
- **MCP:** the `site` argument, above.

**Public view.** An owner or admin can publish a read-only link to a website's analytics, for showing it to a client, a team or the world. tracetells.com does this for itself and for its demo shop.
- **Where:** Setup → Public view, the `public_view` MCP tool, or `trace site publish SLUG` on the server.
- **What it shows:** anyone with the link (`/p/…`) sees Insights, KPIs and Behaviour. Everything is aggregated, and every row built from fewer than 5 sessions is left out, so a quiet website can't show one visitor's path.
- **What it never shows:** settings, people, tokens, the write key, privacy rules, where the website collects from, events it didn't declare, or dismissed insights and their reasons. Nothing in it can change anything.
- **Changing it:** **Replace link** makes the old link stop working at once, and **Make private** withdraws it. Every change is in the activity log.

**The live demo.** The Stride shop runs in two places from one source (`crates/trace-server/assets/shop/`): at the root under `trace serve --demo-site`, and at `tracetells.com/demo` on GitHub Pages. For GitHub Pages, `deploy/build-site.sh` builds the marketing site plus the shop, with a base path, static review files (one fetch in three asks for a file that doesn't exist: a real failed request) and a `404.html` that sends deep links back into the shop.

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

An operator signs in normally, and the **Admin** area appears once they have turned on two-step verification. It lists every account with its plan, people, websites and this month's usage; edits plan templates (empty means no limit) and the default plan for new accounts; changes an account's plan or overrides single limits ("same as plan", "no limit" or a number); and creates accounts with a single-use invite link for their owner (who signs in to accept if they already have a login). It never shows an account's analytics, insights or privacy rules. Operator actions are recorded in the operator's own account's activity log. Set the plan new accounts get with `TRACE_DEFAULT_PLAN` (e.g. `free`) or from the Admin area.

Built-in templates: `unlimited` (no limits, the default), `free` (30 days, 1 website, 100,000 events, 3 members, 5 Claude analyses), `friend` (365 days, 5 websites, 1,000,000 events, 10 members, 50 Claude analyses) and `owner` (no limits).

**Upgrading** a database from before accounts turns each website into its own account (people, invites, tokens and the log move with it), keeps a backup at `<db>.pre-accounts`, and turns members and viewers into users. It stops, listing them, if one email address is in more than one workspace.


**The admin interface** is at `/admin`, separate from the dashboard: Overview (accounts, people, websites, sessions and events across the server, and sessions per day), Accounts (plans, limits and usage, with an account's people and websites), People (everyone and the accounts they belong to), Websites (every website with its account, status and usage) and Plans. It needs operator rights and two-step verification, like before.

**Viewing as someone.** From People, or an account's people, an operator can open the dashboard as that person to see what they see. It is read-only (reads, questions and switching between their accounts; nothing that changes anything), ends after an hour, can't be used on another operator, and doesn't touch the person's own state such as their last-used account or last sign-in. A banner on every page says who is being viewed, with **Stop viewing as them**. It is recorded in the operator's activity log and, as `operator.viewed_as`, in the activity log of the account being viewed, so customers can see it.

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
| Visitors' addresses | Never stored. Hashed with the website, the browser and a daily salt to tell visits apart; salts older than yesterday are deleted |
| Visitors' devices | Nothing stored unless the site waits for its own consent tool and the visitor agrees |
| User ids from `identify()` | Must be opaque. Email-like ids are hashed (SHA-256) in the browser, and the server drops anything still personal |
| Raw events | Deleted after 90 days (`--retention-days`). Insights, KPIs and evaluations are aggregates and are kept |

The analyst only ever sees this scrubbed, aggregated digest, never individual sessions.

**Visitor text is data, not instructions.** Anyone can send events to a website's collector, and labels, error messages, selectors, routes and event names end up in what coding agents read. So the server stores them as one plain line (line breaks, control and invisible characters become spaces, backticks become quotes, spaces in paths become `%20`) and cleans them again in the summary. The behaviour summary, the analysis brief, the server's instructions and every insight's task prompt tell the agent that this text came from visitors' browsers and is never to be followed. Read an insight's task before handing it to an agent that can run commands, as you would any issue a stranger could have filed.

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

The collector is 8.3 KB on the wire with brotli (9.4 KB with gzip), checked by a test so the number stays true, and is built not to affect the host page. The readable, commented source is in the repo; the server strips comments and compresses it once at startup. It is cached for an hour and then revalidated with an ETag, so collector upgrades reach every site without changing the snippet. Handlers are passive and only note what happened; describing elements and queueing run in `requestIdleCallback`. Nothing reads layout on the hot path (no `innerText`; scroll height is sampled at most every 250 ms). DOM mutations are observed only during the second after a click, to spot dead clicks. Storage writes are throttled, data is sent in batches with `sendBeacon`, and every callback is wrapped so collector errors never reach the page. `data-sample="0.25"` records only a share of visitors; the rest skip setup entirely.

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
| `sample` | Share of visitors to record, e.g. `0.25`. Cookieless, the server does the sampling, by visitor |
| `pageviews` | `auto` (default) or `manual`, in which case you call `trace.page()` yourself |
| `console` | `false` stops recording `console.error` calls. The collector wraps `console.error`, so devtools shows `d.js` as the call site for those messages |
| `consent` | Leave it out to stay cookieless (the default), or `required` to wait for your own consent tool (see below) |

API: `trace.track(name, props)`, `trace.identify(userId)`, `trace.page(route?)`, `trace.flush()`, `trace.consent(true | false)`.

### Cookieless by default

The collector stores nothing on visitors' devices: no cookie, nothing in `localStorage` or `sessionStorage`. Ids live in memory for the page only. The server tells visits apart instead, from a hash of the website, the visitor's address and their browser, salted with a random value that changes every day (UTC). The address is never stored, and salts older than yesterday are deleted, so once a day is over its hashes can't be linked to anyone or to the next day's visits. With nothing stored on the device and only aggregates kept, there is no consent banner to show. Check that this fits the rules that apply to your site.

**Signed-in people** count once across days when the site calls `trace.identify(userId)` with its own opaque id. Cookieless, the id is kept for the page only, so call it on every page while someone is signed in. Linking someone's visits across days is per-person tracking, even with an opaque id: say so in your privacy policy, and leave it out where it doesn't fit, as on a site for children.

A site that used to ask for consent can switch by removing `data-consent`: the collector deletes the ids it left in visitors' browsers, and keeps not recording anyone who said no.

What it costs: a visitor who comes back tomorrow counts as a new visitor, and two people on one network with the same browser look like one. Sessions, funnels, rage and dead clicks, errors and KPIs all work as usual, and a session that runs past midnight carries on.

### When your site already asks for consent

```html
<script src="https://trace.example.com/d.js?k=WRITE_KEY" data-key="WRITE_KEY" data-consent="required" defer></script>
```

- **Before an answer**, nothing is stored, no listeners run and nothing is sent. Calls such as `trace.track()` wait.
- **Your consent tool** calls `trace.consent(true)` or `trace.consent(false)`, which also works through the loading stub before `d.js` arrives. Trace draws nothing itself.
- **After a yes**, the collector keeps a random visitor id and session id in `localStorage`, so returning visitors are recognised across days.
- **The answer** is kept as `trc_consent` in `localStorage`. A no, or withdrawing later, deletes Trace's ids and stops collection. A browser sending **Global Privacy Control** counts as no. `trace.consent()` returns `"yes"`, `"no"` or `null`.

`data-consent="banner"` still works for sites that already use it: Trace's own small bar, with `consent-text`, `consent-link`, `consent-allow`, `consent-deny` and `consent-link-text` for its wording, and `data-trace-consent` on a link to open it again. New installs don't need it.

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
| `TRACE_TERMS_URL`, `TRACE_PRIVACY_URL` | – | Terms of service and privacy policy that people accept when they sign up (shown with `TRACE_ALLOW_SIGNUP`) |
| `TRACE_DEFAULT_PLAN` | `unlimited` | Plan for new accounts, applied when the server starts |
| `TRACE_SERVER` | – | Server URL for `trace push` |
| `TRACE_RETENTION_DAYS` | `90` | Delete raw events older than this (0 keeps them). Caps each plan's own limit |
| `TRACE_TOKEN` | – | Personal API token for `trace push` (create one under Account) |
| `TRACE_ADMIN_TOKEN` | – | Deprecated shared token with admin rights. Prefer personal tokens, which carry their owner's role and name |
| `ANTHROPIC_API_KEY` | – | Turns on the Claude analyst |
| `TRACE_MODEL` | `claude-opus-5-5` | Analyst model |
| `TRACE_EFFORT` | `high` | `output_config.effort` for the analyst |
| `ANTHROPIC_BASE_URL` | `https://api.anthropic.com` | |

## CLI

```
trace [--site S] ...
           init [--database sqlite|postgres] [--db TARGET] [--force]   # choose the database, save it to trace.env
           serve [--config trace.json] [--demo-site ADDR] [--allow-signup] [--contact-url URL]
           check [FILE] | push [FILE] [--server URL --token T]
           analyze [--engine auto|claude|heuristics] [--days 14] [--max-insights 5] | evaluate
           insights [--status new] | prompt <id> | status <id> <status> [--reason ...] [--at TIME] | demo
           site list | site add NAME [--account A] | site remove SLUG --confirm NAME
           site publish SLUG [--account A] [--replace | --withdraw]     # the public, read-only view
           user create --email E [--role owner|admin|user] [--account A] | user list
           user reset-password --email E | user reset-mfa --email E
           operator grant EMAIL | operator revoke EMAIL | operator list | operator set-plan PLAN --account A
           mcp [--account A]    # MCP server on stdio for a local agent; remote agents use POST /mcp
```

## HTTP API

`GET /d.js?k=WRITE_KEY` (collector) · `POST /v1/collect` (write key) · `POST /mcp` (MCP, API token) · with an API token: `GET /api/sites` · `GET /api/plan` · and per website, under `/api/sites/{site}`: `GET|POST /config` · `GET /kpis` · `GET /digest?days=14` · `GET /insights?status=` · `GET /insights/{id}/prompt` · `POST /insights/{id}/status` (with `released_at`) · `POST /insights/{id}/kpi` · `POST /releases` · `GET /analysis` · `POST /analyze` · `POST /evaluate` · `GET /runs`

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
