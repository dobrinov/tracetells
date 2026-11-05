# Design

## Context

- **Data.** The collector already sends everything a page story needs. Events (`events` table) carry `session_id`, `ts`, `type`, `path` (the route name the collector computed, e.g. `/questions/:id`), `selector` (up to 4 levels, from `selector()` in `assets/d.js`), `text` (scrubbed label), `name` (custom events and error kinds) and `value` (scroll depth for `scroll`, time on page for `leave`).
- **Existing aggregates.** `trace-core/src/digest.rs` builds the website-wide digest the dashboard's Pages page (`assets/app/pages.js`) shows:
  - per-page views, exits, average scroll and average time (`PageStat`)
  - page-to-page `transitions`
  - clicks, rage clicks and dead clicks per page and selector (`ClickStat`)

  It merges nothing below a threshold. `query.rs` answers behaviour queries and merges breakdown rows under 3 sessions.
- **Thresholds today.** `public.rs` (`MIN_SESSIONS = 5`) drops public-view rows from fewer than 5 sessions. `areas.rs` calls an area under 10 sessions "too little data".
- **Collector.** `d.js` finds its server from `document.currentScript` (`SCRIPT_SRC`) and posts to `/v1/collect`, which has a permissive CORS layer (`api.rs`). Route names come from `config.routes`.
- **Size.** It is 8,372 bytes brotli and 9,502 gzip, against `the_collector_stays_small` budgets of 8,449 and 9,549 that back the published "8.4 KB" and "9.5 KB" claims.
- **Where the website lives.** `event_sources` records the origins the collector has sent from, which is how Trace knows the website's addresses.
- **Dashboard.** Pages are plain scripts in `assets/app/`, registered in `APP_SCRIPTS` (api.rs) and `index.html`. Reports are cached for a minute (`cache.rs`) and dropped on API changes.

## Goals / Non-Goals

**Goals:**
- One builder answers the page story for the dashboard, the API, MCP and the overlay, so they can't disagree.
- The privacy threshold is enforced in that builder, not in each view.
- Building a story for a busy page stays well under a second on SQLite and Postgres.

**Non-Goals:**
- Any new collection: no mouse movement, hover, coordinates, viewport size or DOM capture.
- Recordings, per-session views, or "show me the sessions behind this pattern".
- Comparing two windows or two releases on a page story (later, through release evaluation).
- Changing the dashboard's information architecture. The story page hangs off the existing Pages, area and frustration lists and moves with any later redesign.

## Decisions

### 1. A visit to a page is a run of events with that path
**Choice.** Within a session, ordered by `ts`, a visit to page P is a maximal run of consecutive events whose `path` is P. Its first actions are the visit's events with these types, mapped to steps:
- `click`, `dead_click`, `rage_click`: the element
- `form_start` and `submit`
- `error` (by kind)
- `custom` (by name)

A dead or rage click marks the click step it follows (same selector within 1 s) as "did nothing" instead of adding a step. `scroll`, `leave` and `pageview` are not steps; they feed scroll and time.

**Alternative.** Use the pageview-to-next-pageview window by time. Rejected: single-page apps send events with the route they happened on, so grouping by path is exact and needs no time window.

### 2. Elements are keyed by selector, labelled by the most common text
**Choice.** An element is a `(selector, text)` pair, since one selector can cover several buttons with different labels. Pairs with the same selector are kept apart while each has at least 5 sessions; otherwise they fold into the selector with its most common label.

**Alternative.** Text only. Rejected: icons and inputs have no text.

### 3. Patterns are common runs of steps anywhere in a visit
**Choice.**
- **Steps.** A visit's actions become steps. Actions within 300 ms of each other are one step, as an unordered set, so a submit and the custom event it triggers form one step in either order. Consecutive identical steps become one step marked "repeated". A step's identity ignores whether a click did nothing; each step counts the sessions in which it did nothing.
- **Candidates.** Every run of 1 to 4 consecutive steps in every visit (a single step counts only when most of its sessions repeated it: tapping again after nothing happened is a pattern on its own), each with the sessions that did it, the sessions that did it twice or more, and its average first position.
- **Selection.**
  - Drop runs from fewer than 5 sessions; count the 2-step ones among them (seen in 2 or more sessions) as hidden.
  - Drop runs that start and end on the same step: they are a loop seen from its joint.
  - Drop a run contained in a longer candidate with at least 80% of its sessions.
  - Among rotations of the same loop with similar support, keep the one that comes earliest in visits.
- **Ranking.** Runs where a step did nothing in at least a quarter of their sessions come first ("where it goes wrong"), then by sessions and length, at most 6.

**Why.** On wunderkind.bg's `/questions/:id` (23 sessions), visits split 11 ways at their second step, so grouping visits by how they begin found one pattern ("session_started"). Runs anywhere found "answer square did nothing → answer square" in 6–9 sessions and "number pad → Check → submit" in 8. Every listed run still covers at least 5 sessions.

**Alternatives.**
- A prefix tree of how visits begin (the first plan): exact and simple, but it finds nothing at the traffic most pages get.
- Clustering whole visits by edit distance: shows "typical" visits no 5 sessions actually share.

### 4. The threshold is 5 sessions
**Choice.** Every row in a story needs at least 5 sessions, the public view's rule (`public::MIN_SESSIONS`), reused as a constant rather than the 3 that breakdowns use.

**Why.** Ordered sequences on one page are more distinctive than a single count, so the stricter rule applies. Pages under 5 sessions answer "too little data".

### 5. Bounded work
**Choice.** The builder reads the page's events in the window, newest sessions first, capped at the 2,000 most recent sessions that viewed the page. When the cap applies, the story says so ("based on the latest 2,000 sessions").

It runs as two queries, both using the `(project_id, type, ts)` and `(session_id, ts)` indexes and portable to SQLite and Postgres:
- the sessions that viewed the page in the window
- the events of those sessions, ordered by session and `ts`

Sequences are built in Rust. The result goes through the existing one-minute report cache, keyed by site, path and days.

**Alternative.** Precompute nightly. Rejected for now: no other report does, and the cap keeps cost predictable.

### 6. The overlay is the team's own page plus a script, opened from a fragment token
**Choice.**
- **Minting.** `POST /api/sites/{site}/overlay {url}` checks that the URL's origin is in `event_sources` for the site. It creates a token (32 random bytes, stored as SHA-256 in a new `overlay_tokens` table with site, creator and expiry at 15 minutes) and returns `url#trace-overlay=<token>`.
- **Collector.** At start-up, `d.js` tests `location.hash` for `trace-overlay=`. If present, it:
  - calls `history.replaceState` to drop the fragment
  - appends `<script src="{server}/overlay.js" data-token=…>`
  - returns before installing any handler

  So nothing is collected in that tab and visitors pay only the regex test.
- **Overlay script.** `overlay.js`:
  - computes the route name the same way the collector does (it receives the collector's config through the script element)
  - fetches `GET /v1/overlay/story?route=…&days=…` with the token as a bearer header (CORS as for `/v1/collect`)
  - draws badges in a shadow-DOM layer positioned over `querySelectorAll(selector)` matches, re-placing them on scroll and resize via `requestAnimationFrame`
  - lists unmatched elements
  - plays patterns at a fixed 1.2 s per step

  It never changes the page's own DOM outside its layer.

**Alternatives.**
- An iframe in the dashboard: most websites send `X-Frame-Options` or `frame-ancestors`, and cross-origin frames can't be drawn on.
- Stored page snapshots: forbidden (DOM snapshots).
- A bookmarklet: no collector change, but the team's own clicks would be collected, and it's awkward to install.

### 7. Fitting the collector budget
The overlay branch has 77 bytes brotli and 47 gzip of headroom. Write it as one small guarded block near the start, reusing `SCRIPT_SRC` and the existing `safe()` wrapper. If `the_collector_stays_small` fails, save bytes elsewhere in `d.js` (shorter internal names in rarely-run code) before considering raising the budget. Raising it is out of scope for this change: it would mean changing the README, docs and comparison claims.

### 8. Dashboard and MCP
- **Dashboard.** A new script `assets/app/story.js` serves `#/understand/pages/<path>`:
  - the pattern list with a step-by-step drawing (a schematic of steps, not of the page)
  - the elements table with shares and dead shares
  - arrivals and exits as two bar lists
  - scroll and time
  - "Show on my page", which asks for an address on a known origin, prefilled with the origin plus the path when the route has no parameters

  `pages.js` rows, `features.js` area pages and the rage, dead and error lists link to it.
- **MCP.** `get_page_story {site, path, days}` returns the same JSON with the visitor-text notice the behaviour summary already carries.

### 9. Problems and insights come with the story
**Choice.** The builder adds two lists:
- **Problems.** Dead clicks, rage clicks and errors on the path, grouped as the digest groups them (selector and label, or error kind and message), with events and sessions. They use the digest's rules, not the 5-session floor, because they are the same rows the Pages page shows today and a rare error is still a bug.
- **Insights.** The website's insights whose `pages` contain the path, or a `*` pattern covering it, as summaries (id, title, kind, confidence, status). Dismissed ones go last.

**Not included:** server errors. Their routes are the server's, not the collector's route names, so matching them to a page would guess.

## Risks / Trade-offs

- **Selectors drift as the site changes**, so the overlay misses elements. → The unmatched list shows them, and the story is per window, so after a release the newer selectors take over.
- **Several elements share one selector** (e.g. `div.grid.w-full > button`), so the overlay badges all of them with one number. → The badge says "one of 4 like this", and the elements table keeps the label.
- **Sequences are noisy on pages with many different paths**, leaving few patterns over 5 sessions. → The story still has elements, arrivals and exits. It says how many visits fit a shown pattern ("patterns cover 17 of 23 sessions").
- **Small websites see mostly "hidden".** → Intended. The 5-session rule is what keeps it about patterns. Longer windows (30 or 90 days) are offered.
- **An overlay token leaks via a shared screenshot of the address bar** before the fragment is removed. → 15-minute expiry, read-only, one website, and it only reads aggregates a public link could show anyway.
- **The overlay script runs on the customer's page**, so a bug could break it for the team member. → It only runs in a tab opened from an overlay link, isolated in shadow DOM and wrapped like the collector. Visitors never load it.

## Migration Plan

- `overlay_tokens` is a new table in `schema.sql` and `structure.sql`, added by `db::migrate` on start. Expired rows are deleted by the existing retention sweep.
- To roll back, deploy the previous binary. The table stays unused, and old collectors simply ignore overlay links: the fragment does nothing and the page is collected normally.

## Open Questions

- The pattern cap (6) and step cap (8) are starting values; tune them on wunderkind.bg and Stride once the builder runs. This doesn't change the specs.
