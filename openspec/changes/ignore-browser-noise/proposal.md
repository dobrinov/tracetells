# Proposal

This is a bug fix: Trace records errors that come from visitors' browsers, extensions and blockers rather than from the website, and reports them as the website's errors.

## Why

A team should only see errors it can fix. In the last 30 days (production data, 2026-11-04), the largest group of browser errors on wunderkind.bg was `POST stats.aparatnata.com/api/event → failed`: 196 events, but only in 11 of about 300 sessions. That's another site's analytics endpoint, refused by visitors' ad blockers. It also appears on tracetells.com, and its script shows up as `Failed to load script stats.aparatnata.com/js/:id`. The same window has `Script error.` (7 events, no source, nothing to act on) and a crypto-wallet extension's `window.ethereum` error. The defect sweep lists errors like these as defects, and the analysis reads them as the site's error rate, so they send teams to fix what isn't theirs.

## What Changes

- **Requests to other sites that get no response.** Observed: a request or a script, image or stylesheet load to a different site that fails without an HTTP response is recorded as a network or resource error. Ad and privacy blockers cause exactly this. Expected: it isn't recorded. Failures with an HTTP status, such as `→ 500`, are still recorded, and so are failures to the website's own site and its subdomains. Reproduce: on a page that sends a request to another site, block that site with an ad blocker and load the page.
- **Errors from browser extensions.** Observed: an error thrown in an extension's own script is recorded, with its `chrome-extension://<id>` file as the source when it fires `window.onerror`, or with the page's own frame when it comes through a rejection or `console.error`. Expected: an error whose source or top stack frame is in an extension (`chrome-extension:`, `moz-extension:`, `safari-extension:`, `safari-web-extension:`) is not recorded. The collector never sends the extension's address, since an extension's id helps fingerprint a visitor. Reproduce: install an extension that throws from a content script, and load the page.
- **`Script error.` with no source.** Observed: the browser hides the details of errors from cross-origin scripts, extensions included, and Trace records `Script error.` with nothing to locate or fix. Expected: an exception with the message `Script error.` and no source isn't recorded. Reproduce: load a script from another origin without `crossorigin` that throws.
- **The defect sweep** stops listing such failures as candidates, because they're no longer recorded.
- **Where each rule runs:** extension errors are dropped in the browser, and again on the server for collectors loaded before this change, which browsers keep for up to an hour. The other two rules run on the server alone, because what they need, the request's host and the error's message, is already sent and is safe to receive.

## Capabilities

### New Capabilities
- `browser-errors`: which errors in visitors' browsers Trace records as the website's own, and which it leaves out as noise from outside the website.

### Modified Capabilities
- `defect-sweep`: the failing-request example in "Defect candidates" becomes one on the website's own API, since a third-party request with no response is no longer recorded.

## Privacy impact

Less collected, nothing new. An extension's address is dropped in the browser and never sent; the server drops it again if an older collector sends it. No new data is stored or exposed.

## Site impact

Small. The collector adds one check on an error's source, which runs only when an error happens. It adds no listeners, layout reads or requests. The collector grows by a few tens of bytes and must stay within the 8.4 KB budget; everything else is on the server.

## Impact

- `crates/trace-server/assets/d.js`: error capture (`error`, `frame`).
- `crates/trace-core/src/ingest.rs`: all three rules at ingest.
- `openspec/specs/defect-sweep`: one scenario.
- README and the dashboard's Errors page: what isn't counted.
- Errors already stored stay as recorded; the defect sweep and error rates show them until they leave the window.
