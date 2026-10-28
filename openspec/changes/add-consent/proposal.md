# Proposal

This is a feature.

## Why

The collector keeps a random visitor id and session id in the visitor's `localStorage`. Under the EU's ePrivacy rules, storing anything on a visitor's device usually needs their consent. A privacy-first product has to make asking easy, and its own marketing site must ask before tracking anyone.

## What Changes

- New collector option `consent`:
  - **`banner`** shows a small bar with **Allow** and **No thanks**.
  - **`required`** waits for the site's own consent tool to call `trace.consent()`.
  - Either way, nothing is stored, listened to or sent until the visitor agrees, and queued calls wait.
- **The answer** is kept as `trc_consent` in `localStorage`. **No thanks**, or withdrawing later, deletes Trace's ids and stops collection.
- **Global Privacy Control** counts as no, with no bar shown.
- **Changing your mind:** elements with `data-trace-consent` reopen the bar. `trace.consent()` reports the answer.
- **Wording:** `consent-text` and `consent-link` set the bar's sentence and its "How it works" link.
- **Dashboard:** the install snippets on Setup and Get started get an **Ask visitors for consent first** option, on by default, and the agent install prompt mentions the footer link.

## Capabilities

### New Capabilities
- `consent`: asking visitors before anything is stored or sent, and respecting their answer.

### Modified Capabilities
None.

## Privacy impact

Less data: with consent on, nothing at all is stored or collected before a yes, and nothing after a no. The only thing stored without consent is the answer itself (`trc_consent`), which is strictly necessary to respect it.

## Site impact

About 2 KB more collector code. Before consent, the collector only adds one click listener (for `data-trace-consent` links) and, with `banner`, one small fixed element. No layout reads, and nothing changes for sites that don't turn consent on.

## Impact

`crates/trace-server/assets/d.js` (consent gate, bar, API), `assets/app.js` and `style.css` (snippet option), README.
