# Design

## Context

- **Layout today.** `assets/index.html` has a header with the brand, the site switcher, `#nav` (Home, Understand, Improve, Setup), the collection pill, the Analyse button and the account link, then `#subnav` for an area's pages.
- **Routing today.** `app/core.js` defines `SECTIONS` (four areas and their pages), `ALIASES` for the flat links from before the last redesign, `resolve()` (hash → page and argument), `pageUrl()` and `renderSubnav()`. `app/boot.js` maps page keys to render functions in `routes` and runs `route()`.
- **What each page uses.** Pages are plain scripts sharing one scope, loaded in the order of `APP_SCRIPTS` (api.rs) and `index.html`. Each `render*` function writes `view.innerHTML` from one or more `api()` calls:
  - `renderHome` (home.js): `/today`, `/audience`, `/analysis`
  - `renderAudience` (audience.js)
  - `renderFeatures` and `renderArea` (features.js): `/areas`
  - `renderFunnels` (home.js): `/funnels`
  - `renderBehaviour` (pages.js): `/digest`
  - `renderStory` (story.js): `/pages/story`
  - `renderErrors` and `renderServerErrors` (errors.js)
  - `renderKpis` (home.js)
  - `renderInsights` (insights.js), with an `insightsMode` of insights or releases
  - setup.js, settings.js and accounts.js for Settings
- **Reports.** Every report endpoint is cached for a minute (`cache.rs`), so a screen that calls several is cheap.
- **Home's items.** `today.rs` items carry a bare page key (`"insights"`, `"start"`, `"errors"`) that the dashboard turns into an address.
- **Public view.** `PUBLIC_PAGES` (goals, insights, releases, funnels, pages, errors) decides what `/p/<token>` shows; it reads `/v1/public/<token>/…`.
- **Rules that apply.** The `dashboard` spec forbids the dashboard from changing insights (the agent does, from copied commands) and requires self-measured page names without ids. Markup uses the `html` tagged template. The style uses CSS tokens with light and dark themes (`style.css :root`, `[data-theme]`).

## Goals / Non-Goals

**Goals:**
- One sidebar with six places plus Settings, at stable addresses, with every old link redirected.
- Pulse as a visual first screen built from the reports that already exist, plus one small addition (sources).
- Five stages that each ship and leave the dashboard whole. After stage 1, places that aren't redesigned yet show today's page.

**Non-Goals:**
- Changing what any report computes, except adding sources to the audience report.
- New write actions in the dashboard. The Work board shows commands for the agent, as insight cards do today.
- The canvas's exact colours and type. The dashboard keeps its tokens, both themes and its fonts; the canvas gives the layout and the visual forms (bars against a target line, step columns, shaded tables, a map).
- A separate Audience place. Its tiles go on Pulse, and its full page stays reachable from Pulse's traffic band (`#/pulse/audience`).

## Decisions

### 1. Places replace areas in one table
**Choice.** `SECTIONS` in core.js becomes `PLACES`, a list of `{ key, label, icon, views }`. The six places and Settings each own their views: Product map owns areas, an area, pages and a page story; Settings owns start, install, privacy, team and plan.
- **Addresses.** `resolve()` reads `#/<place>/<view>/<arg>`.
- **Redirects.** One `REDIRECTS` table maps every old key and old area/page pair to a place and view: the flat aliases, the four areas, and Home's bare item keys. `route()` replaces the address with the canonical one, as it does now.

The sidebar is rendered from `PLACES`, so adding a place is one entry.

**Alternative.** Keep `SECTIONS` and add a sidebar on top. Rejected: two navigation models would disagree about where a page lives.

### 2. The sidebar is the layout's left column
**Choice.** `index.html` gets a `<nav class="places">` before `<main>` in a `flex-wrap` row. The sidebar holds:
- the brand, the website switcher (moved from the header) and the collection status
- the places with their counts
- Settings and the account at the bottom

The content column is `flex: 999 1 560px; min-width: 0`. Below 760 px the sidebar collapses (CSS only) into a top bar with the switcher and a menu button, plus a fixed bottom bar with five places. `main` gets bottom padding so content clears that bar. The Analyse button moves into Pulse's header and into Work.

**Sidebar counts.**
- **Problems:** from the problems report (stage 2), or before that from `/digest` (pages with any rage, dead or error row).
- **Goals:** from `/kpis`.
- **Work:** from `/insights?status=new`.

They load after the page renders, so the page never waits for them.

**Alternative.** A top bar with the six places. Rejected: on the canvas the vertical list reads as "where am I" and leaves room for counts; six labels plus a switcher crowd a laptop header.

### 3. Pulse is composed in the browser from existing reports
**Choice.** `app/pulse.js` calls `/today`, `/audience?days=N`, `/kpis?days=N`, `/funnels?days=N`, `/areas?days=N` and `/analysis` in parallel (each cached for a minute), then draws six parts. A part whose report fails shows a one-line notice, and the others still draw.
- **Summary.** One sentence written from those numbers by a small, tested function that leaves out any missing part.
- **Traffic tiles.**
  - Sessions, with `audience.visitors.days` as bars.
  - Top source, from the new `audience.sources`.
  - Phones, from devices in `audience.bounce.by_device`, falling back to `/digest`'s devices.
  - Leaving after one page, from `audience.bounce.rate`.
- **Goals.** A goal's bar width is `min(current / target, 1.25) / 1.25`, so the target line sits at 80% of every track. For "decrease" goals the ratio inverts (target / current). Goals under 10 sessions count as collecting, as on Goals today.
- **Next up.** The first three `/today` items, using the existing `analysisPanel` when analysis is due.
- **Funnels.** Up to two funnels with sessions, as small step columns with the biggest drop in the "bad" colour.
- **Product map.** The areas as a treemap (decision 6) in miniature.

**Alternative.** A server-side `/pulse` endpoint. Rejected for now: the parts are already cached reports, and composing in the browser keeps the server unchanged except for sources.

### 4. Sources in the audience report
**Choice.** `audience.rs` groups `sessions.referrer` (already an origin, or NULL) over the window:
- **Normalising.** Strip the scheme and a leading `www.` or `m.`; known search engines (google.*, bing.com, duckduckgo.com, yahoo.com, yandex.*, ecosia.org) become one source with a display name.
- **Small sources.** Sources under 5 sessions join "other"; NULL is "direct".
- **Comparison.** Each source also gets its share for the window before.

The result is added as `sources: [{ name, sessions, share, previous_share }]`. It is used by Pulse, and by the Audience page as a small table.

### 5. New reports for Problems and Journeys (stages 2 and 3)
**Choice.**
- **`problems.rs`** in trace-core builds the per-page problems report: per path, counts of `dead_click`, `rage_click` and `error` and the distinct sessions with any of them, top 50 by sessions. It lives next to `digest.rs` and reuses `story::problems` for one page's detail. It is served at `GET /api/sites/{site}/problems?days=` through the report cache, with open server errors from `errors.rs`.
- **`funnel.rs`** gains arrivals and exits: sessions by `entry_path`, and by each session's last pageview path (the digest's `exits` query). For each entry page it adds how many of those sessions had one pageview. Rows under 5 sessions join "other". This is served at `GET /api/sites/{site}/journeys?days=`.

Both queries use the existing `(project_id, type, ts)` and `sessions_project_started` indexes and run on SQLite and Postgres.

### 6. The treemap is drawn with CSS grid from a squarified layout
**Choice.** `app/map.js` lays areas out with a small squarified-treemap function: rectangles in a 0–1 box from session counts. It renders them as absolutely positioned `<a>` tiles inside a box with a fixed aspect ratio, each tile linking to its area. The function is used both full size on Product map and in miniature on Pulse.

Each tile carries a text status as well as its colour. The colours come from the existing tokens: `--bad` / `--bad-soft`, an amber for little used, and `--good-soft` for healthy. Tiles too small for a label get a `title` and appear in a list under the map.

**Alternative.** A CSS-only grid with spans, as on the canvas. Rejected: real area counts vary too much for fixed spans.

### 7. Work reuses insight data and card commands
**Choice.** `app/work.js` reads `/insights` once and sorts the insights into the five columns by status and `evaluation.settled_at`. Cards are a compact version of `insightCard`.
- **Measuring.** The day count is `(now − released_at) / 1 day` out of 14.
- **Measured.** Before and after values are `evaluation.baseline` and `evaluation.current`, with the verdict.
- **Commands.** The next-step command comes from the same helper the cards use today. Opening a card opens the full insight in a side panel, reusing `insightCard`.
- **Dismissed.** Hidden behind a toggle.
- **Old addresses.** `#/improve/insights` and `#/improve/releases` redirect to `#/work`, scrolled to the matching column.

### 8. Self-measurement names
**Choice.** `measurePage` names pages `/<place>` and `/<place>/<view>`, with fixed view names: `/map/area`, `/map/story`, `/settings/privacy`. The address's argument (an area key, a path) is never part of the name.

## Risks / Trade-offs

- **Pulse makes six requests.** → They run in parallel and are each cached for a minute; Pulse draws each part as it arrives.
- **People used to Understand / Improve / Setup lose their bearings.** → Every old link redirects. For a week after release, the sidebar shows a one-line "Where did things go?" note linking to a short map of old to new.
- **The treemap is unreadable with many small areas.** → Areas under 3% of sessions are grouped into one "smaller areas" tile, listed below the map.
- **Each stage leaves some places showing today's pages.** → Each place gets a header in the new style from stage 1, so the old pages sit inside the new layout without looking broken.
- **Normalising search engines can hide a referrer someone cares about** (e.g. google.bg vs google.com). → The Audience page lists sources with their origins under each name.

## Migration Plan

- There are no database migrations; sources and the new reports read existing columns.
- Each stage ships alone in this order: 1 navigation and Pulse, 2 Problems, 3 Journeys, 4 Work, 5 Product map. Deploy with `/opt/apps/trace/deploy.sh`.
- Rollback is the previous binary. Addresses in the new form (`#/pulse`) fall back to Home in an old binary, because unknown pages already land there.

## Open Questions

- The search-engine list and display names (decision 4) can grow from what production websites actually see. This doesn't change the specs.
