# Telltale

Behaviour analytics built for agents. Telltale collects how people use your product, an analyst agent turns that into specific improvement ideas tied to your KPIs, you copy an idea as a prompt for your coding agent, and once it ships Telltale measures whether it worked.

```
collect ──▶ digest ──▶ analyst ──▶ ideas ──▶ prompt ──▶ ship ──▶ evaluate
 (t.js)    (SQLite)   (Claude or   new → selected → in progress → released
                       heuristics)                      ╰── dismissed (with a reason the analyst learns from)
```

## Try it in a minute

```sh
cargo build --release
./target/release/telltale --db demo.db demo       # 30 days of simulated traffic for "Stride", a demo shoe shop
./target/release/telltale --db demo.db analyze    # create ideas
./target/release/telltale --db demo.db serve --demo-site 127.0.0.1:4319
```

Open the dashboard at http://127.0.0.1:4318, and the Stride shop, instrumented with the collector, at http://127.0.0.1:4319. The shop has four planted problems: product images that look clickable but aren't, a "Pay now" button that takes 3 seconds with no feedback, a pricing toggle that throws, and a signup form that wipes itself on a bad phone number. Your clicks show up in the dashboard next to the simulated history.

## Pieces

| | |
|---|---|
| **Collector** (`/t.js`) | One script tag. Captures pageviews (SPA-aware), clicks, rage clicks, dead clicks, JS errors, scroll depth, time on page, form starts and submits, plus `telltale.track(name, props)`. It never records input values; elements inside `data-telltale-mask` have their text dropped. Use `data-telltale="name"` for stable element names. |
| **KPIs** | `conversion` (sessions doing A that later do B), `session_rate` (share of sessions with an event) and `per_session` (events per session). Events are matched by type, path, selector and custom event name; patterns ending in `*` match by prefix. Each KPI has a direction and an optional target. |
| **Analyst** | Builds an aggregated digest and asks Claude for ideas through structured output. It sends your KPIs plus existing and dismissed ideas so it doesn't repeat itself. Without an API key, or when Claude fails, built-in heuristics take over. |
| **Prompts** | Each idea is rendered as a self-contained task for a coding agent: problem, evidence, goal KPI, proposal, routes and acceptance criteria. Copy it from the dashboard, or run `telltale prompt 12 \| claude`. |
| **Evaluation** | For released ideas, compares the KPI in equal windows before and after the release (Welch test, at least 30 sessions per side) and gives a verdict: improved, regressed, no change or insufficient data. It runs every hour while the server is up. |

## Performance

The collector is about 5 KB gzipped, comments included, and is built not to affect the host page. Handlers are passive and only note what happened; describing elements and queueing run in `requestIdleCallback`. Nothing reads layout on the hot path (no `innerText`; scroll height is sampled at most every 250 ms). DOM mutations are observed only during the second after a click, to spot dead clicks. Storage writes are throttled, data is sent in batches with `sendBeacon`, and every callback is wrapped so collector errors never reach the page. `data-sample="0.25"` records only a share of visitors; the rest skip setup entirely.

## Install the collector

```html
<script src="https://telltale.example.com/t.js" data-key="WRITE_KEY" defer></script>
```

The write key is on the dashboard's Setup page. The collector sends JSON as `text/plain` through `sendBeacon`, so browsers skip the CORS preflight.

## Configuration

| Variable | Default | |
|---|---|---|
| `TELLTALE_DB` | `telltale.db` | SQLite file |
| `TELLTALE_ADDR` | `127.0.0.1:4318` | Listen address |
| `TELLTALE_ADMIN_TOKEN` | – | Bearer token for the dashboard API. Set it whenever the server is reachable by others. |
| `ANTHROPIC_API_KEY` | – | Turns on the Claude analyst |
| `TELLTALE_MODEL` | `claude-opus-5-5` | Analyst model |
| `TELLTALE_EFFORT` | `high` | `output_config.effort` for the analyst |
| `ANTHROPIC_BASE_URL` | `https://api.anthropic.com` | |

On the first-party API, the analyst opts into server-side refusal fallbacks (`fallbacks: "default"`), so a request the main model declines is retried on Anthropic's recommended fallback model.

## CLI

```
telltale serve | analyze [--engine auto|claude|heuristics] [--days 14] [--max-ideas 5]
         evaluate | ideas [--status new] | prompt <id> | status <id> <status> [--reason ...] | demo
```

## HTTP API

`POST /v1/collect` (collector) · `GET /api/digest?days=14` · `GET|POST /api/kpis` · `PUT|DELETE /api/kpis/{id}` · `GET /api/ideas?status=` · `GET /api/ideas/{id}/prompt` · `POST /api/ideas/{id}/status` · `POST /api/ideas/{id}/kpi` · `POST /api/analyze` · `POST /api/evaluate` · `GET /api/runs`

## Layout

- `crates/telltale-core`: storage, ingestion, KPI measurement, digest, idea lifecycle, prompt rendering, evaluation
- `crates/telltale-agent`: analyst (Claude client and heuristics)
- `crates/telltale-server`: the `telltale` binary (HTTP server, CLI, demo seeder, embedded dashboard and collector)
