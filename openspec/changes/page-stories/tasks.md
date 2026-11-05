# Tasks

## 1. Page story builder (trace-core)

- [x] 1.1 Add `trace-core/src/story.rs` with the `PageStory` types (elements, patterns, hidden totals, arrivals, exits, scroll shares, time medians and quartiles, `capped`) and register it in `lib.rs`; verify `cargo build --workspace` succeeds
- [x] 1.2 Load the page's sessions (newest 2,000 in the window) and their events in two queries that run on SQLite and Postgres; verify with a test that seeds 2,100 sessions and checks the story reports `capped` and 2,000 sessions on both databases
- [x] 1.3 Turn each session's runs of events on the path into visits and steps, folding dead and rage clicks into the click they follow; verify with unit tests over hand-built event lists (a dead click marks its click; scroll, leave and pageview are not steps)
- [x] 1.4 Collapse repeated steps and repeated blocks of 2–3 steps, cap at 8 steps; verify unit tests for "a a a", "a b a b a b" and "a b c a b c" collapse as the design says
- [x] 1.5 Build the prefix tree (replaced by 8.1: runs anywhere in a visit), prune nodes under 5 sessions, emit up to 6 patterns and the hidden count; verify a test reproducing the spec scenarios: 10 sessions "pad → Check → Next" repeated, 7 "square (did nothing) → square", and patterns of 3 and 2 sessions reported only as "2 hidden"
- [x] 1.6 Compute elements (selector and label pairs, shares, median clicks, dead share, 5-session floor, 50 max) and arrivals and exits (including outside the website), scroll shares and time medians and quartiles; verify tests for the answer-square scenario (11 of 23, 10 dead) and the landing-page scenario (98 arrive, 79 leave)
- [x] 1.7 Return "too little data" for pages under 5 sessions, and assert in a test that the serialized story contains no session id, visitor id, user id, user agent or event timestamp for any input

- [x] 1.8 Add the page's problems (dead and rage clicks by element, browser errors by kind and message, with events and sessions) and its insights (same path or a covering `*` pattern, dismissed last) to the story; verify tests for the yourScore error in 2 sessions being listed and an insight naming `/challenges/:id` appearing on that page's story but not on another's

## 2. Overlay tokens (storage)

- [x] 2.1 Add the `overlay_tokens` table (hash, project, creator, created, expires) to `schema.sql` and `structure.sql` and to `db::migrate`; verify the schema comparison test passes and a fresh database and an existing one both migrate on SQLite and Postgres
- [x] 2.2 Add mint, check and expire functions (32 random bytes, SHA-256 stored, 15 minutes) and delete expired rows in the retention sweep; verify tests for a valid token, a token for another website, an expired token, and the sweep removing it

## 3. API and MCP (trace-server)

- [x] 3.1 Add `GET /api/sites/{site}/pages/story?path=&days=` through the one-minute report cache; verify an API test for members and tokens, and for days outside 1–90 being refused
- [x] 3.2 Add `POST /api/sites/{site}/overlay {url}`, checking the origin against `event_sources`; verify tests for a known origin (returns `url#trace-overlay=…`) and an unknown one (refused, listing the known origins)
- [x] 3.3 Add `GET /v1/overlay/story?route=&days=` with bearer overlay token and CORS, and serve `/overlay.js`; verify tests: a valid token gets the same story as 3.1, an expired one gets 401, and a token for website A can't read website B
- [x] 3.4 Add the MCP tool `get_page_story` for every token including read-only, with the visitor-text notice; verify an MCP test and that the tool appears in the tool list; document it in the plugin's tool reference

## 4. Collector overlay mode

- [x] 4.1 In `assets/d.js`, at start-up, detect `trace-overlay=` in `location.hash`, remove the fragment with `history.replaceState`, load `overlay.js` from the collector's server with the token and config, and return before installing handlers; there is no JS test harness, so verify in a browser on the local demo shop: with an overlay link the network panel shows no `/v1/collect` request and the address bar loses the fragment, and without it events arrive as before
- [x] 4.2 Keep the collector within budget; verify `cargo test -p trace-server the_collector_stays_small` passes with the budgets unchanged (save bytes elsewhere in `d.js` if needed)

## 5. Overlay script

- [x] 5.1 Write `assets/overlay.js` (ES5, no dependencies): compute the route as the collector does, fetch the story, and draw badges in a shadow-DOM layer over `querySelectorAll` matches, re-placed on scroll and resize; verify by opening an overlay link on the demo shop (`assets/shop`) locally and seeing badges on its elements
- [x] 5.2 Add the "Use" and "Dead taps" views, the "not on this page now" list, "one of N like this" for shared selectors, the expired-link message, and pattern playback at 1.2 s per step; verify on the demo shop that a pattern plays step by step, and that an expired link shows the message and draws nothing
- [x] 5.3 Check that the overlay sends no request to Trace other than the story; verify in the browser's network panel on the demo shop

## 6. Dashboard

- [x] 6.1 Add `assets/app/story.js` for `#/understand/pages/<path>`, listed in `APP_SCRIPTS` and `index.html`, using the `html` template: patterns with step-by-step playback, elements table, arrivals and exits, scroll and time, "too little data" and hidden totals; with sections for the page's problems and insights (each insight linking to its page); verify after rebuilding that the demo site's busiest page shows its story, problems and insights
- [x] 6.2 Add "Show on my page": ask for an address on a known origin (prefilled when the route has no parameters), mint the link and open it in a new tab; verify the link opens the demo shop with the overlay drawn
- [x] 6.3 Link pages in `pages.js` (Pages table, rage, dead and error lists), `features.js` area pages and `errors.js` to their story for the same window; verify each link lands on the right story
- [x] 6.4 Update dashboard copy, the README and the homepage section on what Trace shows (patterns per page, never recordings), and the install docs on overlay links; verify the claims match the behaviour above and the collector size claims are unchanged

## 8. Patterns anywhere in a visit

- [x] 8.1 Replace the prefix tree with runs of 2–4 steps anywhere in a visit: same-moment actions as one step, repeats folded, loops and rotations deduplicated, contained runs dropped, wrong-going runs first; verify unit tests for the spec's four Common patterns scenarios
- [x] 8.2 Show in the dashboard and the overlay which step did nothing in how many of the pattern's sessions, and which patterns are done repeatedly; verify on the demo shop that a pattern with a dead step shows "did nothing in N of M"

## 7. Integration

- [x] 7.1 Run `cargo test --workspace` on SQLite and with `TRACE_TEST_POSTGRES` set, and `cargo clippy --workspace --all-targets -- -D warnings -A clippy::type_complexity -A clippy::result_large_err`; verify all pass
- [x] 7.2 On a copy of the production database (read-only), build the story of wunderkind.bg's `/questions/:id` for 7 days and check it shows the "tap a square twice" pattern (about 7 sessions) and hides the patterns under 5 sessions

## Workflow follow-up

- Deploy after review (`/opt/apps/trace/deploy.sh`, with the user's go-ahead) and archive the change once deployed.
