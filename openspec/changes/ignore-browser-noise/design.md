# Design

## Context

**How the collector records errors** (`crates/trace-server/assets/d.js`). Everything goes through `error(kind, message, source)`, which pushes `{type: "error", name: kind, text: message, sel: source}`. The kinds:
- **`network`**: from the `fetch` and `XMLHttpRequest` wrappers, via `networkError()`. The text is `METHOD <shortUrl> → <status or "failed">`. `shortUrl()` keeps the host only when it differs from the page's origin, so `POST /api/x → failed` is the page's own origin and `POST stats.example.org/api/event → failed` is another one. `AbortError` and the collector's own endpoint are already skipped.
- **`resource`**: from the capturing `error` listener when an element fails to load. The text is `Failed to load <tag> <shortUrl>`, with no status, because the browser exposes none.
- **`exception`**: from `window.onerror`. The source is `e.filename` with the page's origin cut off, plus `:lineno`. An extension's script keeps its full `chrome-extension://<id>/…` address. A cross-origin script without `crossorigin` gives `Script error.` with no filename.
- **`rejection`** and **`console`**: the source is `frame()`, the first stack line matching `(https?://|/)…:line`. That regex skips extension frames, so an extension's rejection gets the next page frame, or no source.

**How the server stores them** (`crates/trace-core/src/ingest.rs`, `store`). The server takes the batch with the page's origin (from `Origin` or `Referer`), redacts every field, and inserts each event. It already drops whole batches from developer machines once a website is live. It has no per-event drop rule today.

**Production, last 30 days.** No stored error names an extension address, so the leak in the `exception` path hasn't happened yet. The blocked third-party requests are real and are the largest group (see the proposal).

## Goals / Non-Goals

**Goals:**
- None of the three kinds of noise is stored, whichever collector version sent it.
- No extension address ever leaves the browser.
- No collector bytes spent on rules the server can apply.

**Non-Goals:**
- Errors from code that extensions inject into the page itself (like `window.ethereum`): they run as page scripts, with the page's own file and line, and nothing reliable tells them apart. A site-defined ignore list in trace.json can cover them later.
- Deleting the noise already stored. It leaves the 30-day window by early December. A one-off delete is a separate decision for the owner (task 5.2).
- Telling a blocked request apart from a third-party site that is really down: neither returns a response. See Risks.

## Decisions

**Extensions: in the collector, at the one choke point, and again on the server.**
- `frame()`'s regex also matches `chrome-extension://`, `moz-extension://`, `safari-extension://` and `safari-web-extension://` frames, so an extension frame on top of the stack becomes the source instead of being skipped.
- `error()` returns without pushing when the source matches `-extension:`. Every kind passes through it, and `window.onerror`'s `e.filename` already carries the scheme, so one check covers all of them.
- The server drops any error event whose source or message contains `-extension://`, for collectors loaded before this change.
- *Alternative considered:* dropping only on the server. Rejected: the address would still leave the browser, against "scrub in the browser first".

**Blocked third-party requests: on the server.**
- For `network` errors ending in `→ failed` and for all `resource` errors, take the host from the text: the first token after the method or after `Failed to load <tag> `, up to the first `/`. If it starts with `/`, the request was to the page's own origin and is kept.
- Otherwise compare it with the batch's page origin by site: the same host, or both under the same registrable domain. The registrable domain is approximated as the last two labels, or the last three when the top-level label has two letters and the label before it has at most three (`co.uk`, `com.au`). A different site means the event is dropped.
- *Why the server:* the host is already in the text and isn't personal; the server has the page's origin per batch; and it covers every collector already deployed. It costs visitors nothing.
- *Alternative considered:* a public-suffix list. Rejected for now: a few hundred KB of data for an edge case. The approximation errs towards "same site" (it can treat two unrelated `.co.uk` sites as one), which keeps errors rather than losing them.

**`Script error.` with no source: on the server.** Drop an `exception` whose text is exactly `Script error.` and whose source is empty. Exact match only: any message with details, or with a source, is kept.

**Dropped events don't count.** They're removed before insertion, so they count neither toward the monthly event limit nor toward usage, as with other ignored pages.

## Risks / Trade-offs

- **[A third-party service the site depends on goes down, with no response]**: those failures are no longer recorded, so a broken payment widget hosted elsewhere would show up only as its effect (the funnel step nobody reaches) and not as a request error. Accepted: blocked requests are far more common, and the funnel and defect sweep still catch the effect. A request to another site that answers with a status, such as a 500, is still recorded.
- **[A website served from two unrelated domains]**, such as an API on `example-api.net` for `example.com`: its failures with no response are dropped. Accepted for now; a trace.json list of the website's own domains can fix it if it comes up.
- **[An extension that wraps `fetch`]** is on top of the stack of a page's own rejected request, so that rejection is dropped. That's rare, and the network error from the same request is still recorded.
- **[Error rates drop]** on websites with many blocker users, without any change to the site. The README and the Errors page say so.
