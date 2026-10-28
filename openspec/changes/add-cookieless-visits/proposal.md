# Proposal

This is a feature.

## Why

The collector kept a random visitor id and session id in `localStorage`. In the EU, storing anything on a visitor's device for analytics usually needs consent first, so the dashboard's install snippets added Trace's own consent bar to every monitored website by default. The owner doesn't want Trace drawing its own bar on other people's sites. It also cut the data: Trace only saw visitors who pressed Allow, which left small sites without enough sessions to analyse or to judge a release.

## What Changes

- **Cookieless by default.** Without `data-consent`, the collector stores nothing on the device: no cookie, nothing in `localStorage` or `sessionStorage`. Ids live in memory for the page only, and batches are sent without a session or visitor id.
- **The server finds the visit.** For a batch without a session id, it hashes the website, the visitor's address and their browser with a random salt for the day (UTC). The same hash active in the last 30 minutes continues its session, across midnight too; otherwise a new session starts. The address is never stored. Salts live in a new `daily_salts` table, and anything older than yesterday is deleted.
- **Sampling moves to the server** for cookieless batches: the collector sends `sample`, and the server drops visitors outside it before counting them against the quota.
- **Sites that already ask for consent** use `data-consent="required"`: nothing is sent until their consent tool calls `trace.consent(true)`, and after a yes the collector keeps ids in `localStorage` as before. `data-consent="banner"` keeps working for sites that use it but is no longer offered.
- **Switching from consent is clean.** A cookieless collector deletes the ids an earlier consent mode left in the browser, and records nothing from a browser whose stored answer is no.
- **tracetells.com and its demo shop** drop Trace's bar and go cookieless; the privacy policy says so, dated 29 October 2026.
- **Signed-in people count once.** The visitor total counts a session's `trace.identify()` id where there is one, and the daily hash otherwise, so a signed-in person isn't counted again every day. Cookieless, the id lasts for the page, so sites call `identify()` on every page.
- **Install snippets** in the dashboard and in MCP's `get_install_snippet` are cookieless by default, with a "Wait for my consent tool" option instead of Trace's bar.
- **Copy:** README, the homepage and the privacy policy describe the cookieless default.

## Capabilities

### New Capabilities
- `cookieless-visits`: telling visits apart without storing anything on the visitor's device.

### Modified Capabilities
None.

## Privacy impact

- Visitors' devices: nothing stored by default, where before a visitor id and session id were kept after consent.
- The server now uses the visitor's IP address and user-agent to compute a hash. The address is used only in memory and never stored. The hash is stored as the session's `visitor_id`; it is salted with a random value per day, and salts older than yesterday are deleted, so a stored hash can't be linked to an address or to another day's visits. The user-agent was already stored with the session.
- Returning visitors can't be recognised across days, by design.
- The demo shop and every site without `data-consent` become cookieless when their collector refreshes (within the hour). Sites that used the old default had stored ids without asking; that stops.

## Site impact

The collector grows by a few lines and does less work: no storage reads or writes on the default path.

## Impact

- `trace-core`: new `visits.rs`; `ingest.rs` (empty `sid`, `sample`); `daily_salts` in both schemas; an index on `sessions(project_id, visitor_id, last_seen_at)`.
- `trace-server`: `api.rs` (collect), `mcp.rs` (snippet), `assets/d.js`, `assets/app.js`.
- `site/`: homepage and privacy policy. README.
- Visitor counts over more than a day rise, since a returning visitor counts again each day.
