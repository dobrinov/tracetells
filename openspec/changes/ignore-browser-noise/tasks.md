# Tasks

## 1. Collector: extensions

- [x] 1.1 In `d.js`, let `frame()` match `chrome-extension://`, `moz-extension://`, `safari-extension://` and `safari-web-extension://` frames
- [x] 1.2 In `error()`, push nothing when the source contains `-extension:`
- [x] 1.3 Keep the collector ES5; `the_collector_stays_small` passes without raising the budget

## 2. Server: drop noise at ingest

- [x] 2.1 In `ingest::store`, drop error events whose source or message contains `-extension://`
- [x] 2.2 Drop `network` errors ending in `→ failed` and all `resource` errors whose host is on a different site from the batch's page origin (same host, or the same registrable domain as approximated in design.md)
- [x] 2.3 Drop `exception` errors whose message is exactly `Script error.` and that have no source
- [x] 2.4 Dropped events count neither toward the monthly limit nor toward usage (the collect endpoint now gives back the quota for every event `store` leaves out, which also covers ignored pages and developer machines)

## 3. Tests

- [x] 3.1 Ingest tests, run on SQLite and Postgres: a blocked request to another site is dropped; one to a subdomain of the page's site is kept; one to another site with a status is kept; a same-origin one with no host is kept; a `co.uk` page and a request to another host under the same domain is kept
- [x] 3.2 Ingest tests: an exception from a `chrome-extension://` source is dropped, and so is a rejection whose message names `moz-extension://`; `Script error.` with no source is dropped and with a source is kept
- [x] 3.3 A defect-sweep test where the failing request is on the website's own API, matching the modified scenario
- [x] 3.4 In a browser: a test page dispatching an extension's uncaught error and rejections (Chrome and Firefox stacks) next to the page's own; the collector sends only the page's own, and nothing naming an extension (the server side is covered by the ingest test on both databases)

## 4. Docs

- [x] 4.1 README (collector and errors sections): which browser errors aren't counted, and that error rates may drop on websites with many blocker users
- [x] 4.2 The dashboard's Errors page says the same in its help text

## 5. Release

- [ ] 5.1 Deploy, then check on wunderkind.bg that no new `stats.aparatnata.com` failures are stored after the deploy, while its own-site errors still are
- [ ] 5.2 Ask the owner whether to delete the noise already stored (the three rules applied to the last 90 days of events), and do it only with their go-ahead
