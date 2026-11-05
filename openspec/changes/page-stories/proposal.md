# Proposal

Kind: feature.

## Why

Trace says a page has dead clicks, but not what people were trying to do there. On wunderkind.bg's `/questions/:id`, the Pages table lists selectors and counts; it takes hand-written SQL to see that in 7 of 23 sessions the first tap on an answer square does nothing and people tap again. Teams who know FullStory-style tools expect to *see* behaviour on a page. Trace can't show recordings, so it should show the same thing as patterns: what most people do on a page, in which order, and where it goes wrong.

## What Changes

- A **page story** for any page with enough sessions: the elements people use (share of sessions, taps per session, share that did nothing), the common orders of action, where people came from and went next, how far they scroll, how long they stay and how long until their first action.
- **Common patterns**: each visit to a page becomes a short sequence of actions (repeats collapsed); visits that begin the same way are grouped into a tree, and only branches shared by at least 5 sessions are shown. Each pattern can be played as an animation on the page.
- **Problems and insights on the page**: the story also lists the page's dead clicks, rage clicks and browser errors, and the insights that name the page, so everything known about one page is in one place.
- **Page overlay**: from a page story, the team opens their own live page with Trace drawing on top of it (badges on the elements people use, the dead-tap view, pattern playback). The page is the team's own browser loading their website: nothing from any visitor's screen is recorded or stored.
- The collector learns one thing: when its page is opened with an overlay link, it collects nothing and loads the overlay script from the Trace server instead.
- The dashboard's Pages table links each page to its story; area pages and frustration lists link to the page they name.
- The MCP tool `get_page_story` gives agents the same story, so `/trace:analyse` can explain a problem with the pattern around it.

## Capabilities

### New Capabilities
- `page-stories`: aggregated behaviour on one page: element use, common action patterns, arrivals and exits, scroll and time, in the dashboard and through MCP, with the minimum-sessions rule that keeps every row a pattern rather than a person.
- `page-overlay`: drawing a page story on the team's own live page from a short-lived overlay link, and the collector's overlay mode.

### Modified Capabilities
None. `behaviour-queries` already forbids single events, session ids and single-event timestamps; page stories follow that rule and add a stricter threshold of their own.

## Privacy impact

- **Collected:** nothing new. Page stories are built from events the collector already sends (clicks with selector and label, dead and rage clicks, form starts and submits, errors, custom events, scroll, leave).
- **Stored:** short-lived overlay tokens (random, hashed, one website, 15 minutes, read-only). No new visitor data.
- **Exposed:** aggregates only. Every element, pattern step and transition shown covers at least 5 sessions; smaller ones are counted together as "hidden". No session ids, timestamps of single events, single visits or per-visit timings are returned. Playback is drawn at a fixed pace from the pattern, never from a visit's real timing. Element labels stay as scrubbed today and are marked as visitor text in MCP output.
- **Overlay:** the overlay link carries its token in the URL fragment, which browsers do not send to the website's server; the overlay removes it from the address bar on load. In overlay mode the collector sends no events, so the team's own looking doesn't count as a visit.

## Site impact

- The collector gains a check of `location.hash` at start-up and a branch that loads one script, roughly 60–90 bytes compressed. Today it is 8,372 bytes brotli and 9,502 gzip against budgets of 8,449 and 9,549, so the branch must fit in 77 and 47 bytes, saving bytes elsewhere in `d.js` if needed. The published "8.4 KB" claim and `the_collector_stays_small` stay as they are.
- Visitors never load the overlay script. No new event handlers, no layout reads, no extra network use for visitors.

## Impact

- `trace-core`: a new page-story builder next to `digest.rs`, reusing its click, transition and page statistics; overlay tokens in `schema.sql` and `structure.sql` (additive).
- `trace-server`: `GET /api/sites/{site}/pages/story`, `POST /api/sites/{site}/overlay` (mint a link), `GET /v1/overlay/story` (CORS, token-authenticated), `/overlay.js` asset, MCP tool `get_page_story`.
- `assets/d.js` (overlay mode), new `assets/overlay.js`, new dashboard script `assets/app/story.js` (added to `APP_SCRIPTS` and `index.html`), links from `pages.js`, `features.js`, `errors.js`.
- README and homepage copy that describe what Trace shows ("patterns, not people").
