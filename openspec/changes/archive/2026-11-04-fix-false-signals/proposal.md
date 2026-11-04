# Proposal

This is a bug fix: ways Trace reports something that didn't happen, or answers a question wrongly without saying why.

## Why

The first analysis of wunderkind.bg ranked its practice area "painful" largely on signals that weren't real, and an agent's error query matched nothing without saying why. Insights built on false dead and rage clicks send teams to fix things that work, which is the opposite of what Trace is for.

Production data (last 30 days, 2026-11-04) shows the size of it. On wunderkind.bg, of about 210 dead clicks on elements seen in 3 or more sessions, 76 are on a canvas, 16 on a control that reacts on press, and about 100 on things that aren't controls at all (`body`, `main`, `section`, `p`, `img`, `svg`, layout `div`s). On tracetells.com, 15 of 29 dead clicks are on `body` or `html`. 38 of wunderkind's 99 rage clicks are on a number pad that works.

## What Changes

- **Dead clicks on controls that react on pointerdown.** Observed: wunderkind's grid-shade widget colours a cell on `pointerdown`; the collector only starts watching for page changes at `click`, sees none, and records a dead click (16 in 7 sessions on `/questions/:id`). Expected: a change made between the press and the click counts as the page reacting. Reproduce: a button whose `pointerdown` handler changes its class and whose click handler does nothing; click it.
- **Dead clicks on drawing surfaces.** Observed: drawing on a `<canvas>` scratch pad changes pixels, not the DOM, and is recorded as a dead click (76 in 6 sessions). Expected: clicks on a `<canvas>` are not judged dead, as form fields already aren't. Reproduce: click a canvas that draws on pointer events.
- **Rage clicks on controls that respond.** Observed: typing a number on an on-screen number pad (three taps on the same key within 700 ms) is recorded as a rage click (38 in 4 sessions). Expected: a burst counts as rage only when the page didn't react to it. Reproduce: click a button three times quickly whose click updates a readout.
- **Dead clicks on the page itself and on selected text.** Observed: clicks on `body` or `html` (clicking empty space to focus the page or close something) and clicks that select text in a paragraph are recorded as dead clicks: about half of tracetells.com's, and many of wunderkind's on `p`, `main` and `section`. Expected: a click on `html` or `body`, or one that leaves text selected a second later, is not judged dead. Reproduce: click an empty margin of a page; drag across a paragraph to select a sentence.
- **Rage clicks from selecting text.** Observed: triple-clicking to select a line or a field's contents is three clicks within 700 ms and is recorded as a rage click, even in form fields, because rage is decided before the form-field exclusion. Expected: no rage click where the dead-click check doesn't judge (form fields, labels, links that open elsewhere, the page itself) or when text is left selected. Reproduce: triple-click a paragraph, or the text in an input.
- **Error matchers that can't match.** Observed: `{"error": "Missing target*"}` in `query_behaviour` (or in trace.json) is read as a page pattern, matches nothing and returns 0 with no warning; the message belongs in `"text"`. Expected: a page pattern that doesn't start with `/` (and isn't `*`) is refused with a message saying page patterns start with `/` and, for errors and clicks, that the message or label goes in `"text"`. Reproduce: `query_behaviour` with `{"measure": "sessions", "match": {"error": "Missing target*"}}`.

- **Matcher keys that are ignored.** Observed: in the `type` form, keys other than `type`, `path`, `selector`, `name` and `text` are dropped, so `{"type": "error", "message": "X"}` counts every error. Expected: an unknown key is refused, naming the key and the ones allowed. In the shorthand form an extra key is already refused, but with "expected one event type", which doesn't say which key is wrong; it should name it. Reproduce: `query_behaviour` with `{"measure": "events", "match": {"type": "error", "message": "X"}}`.
- **On the hosted server, `"text"` is ignored and counts every error.** Observed (2026-11-04, wunderkind.bg): `{"type": "error", "text": "zzz-no-such-message*"}` returns all 289 errors in 27 sessions, and `{"error": "*", "text": "…"}` is refused with "expected one event type". The hosted server predates text matching (added in `ae4f9d5`, the defect sweep), so there's no working way there to count errors by message, and one way answers wrongly with no warning. Expected: the repo's behaviour, where `"text"` matches the message. This needs a deploy, not a code change, but it's the most harmful of these bugs in production today, so deploying is part of this change (task 5.3), along with `get_defects`, which the plugin's analyse skill already calls.

## Capabilities

### New Capabilities
- `frustration-signals`: when the collector records a dead click or a rage click, and when it doesn't.
- `behaviour-matchers`: how event matchers in trace.json and `query_behaviour` read their patterns, and which patterns are refused.

### Modified Capabilities

None. The existing main specs (`server-errors`, `error-clients`) are unaffected.

## Privacy impact

None. No new data is collected, stored or exposed; the collector records fewer events, not new ones.

## Site impact

Small. The collector adds a passive `pointerdown` listener that only notes the time and the mutation count, and a rage click is decided after the same one-second wait the dead-click check already uses rather than at once. That check also asks whether text is selected (`getSelection()`), which runs a second after the click, not in the handler, and reads no layout. The MutationObserver runs from the press instead of from the click, a few hundred milliseconds longer per click. No layout reads, no new requests. Size grows by about 70 bytes with brotli (8,362) and 70 with gzip (9,478). That passes the old budget of 8,349 / 9,449, so the budget in `the_collector_stays_small` and every published claim move from 8.3 KB to 8.4 KB (9.4 to 9.5 KB with gzip).

## Impact

- `crates/trace-server/assets/d.js`: click handling (dead and rage clicks).
- `crates/trace-core/src/config.rs`: matcher parsing shared by trace.json and `query_behaviour`; `check_config` reports the new refusal.
- `crates/trace-server/src/mcp.rs`: `query_behaviour` description.
- Existing rage and dead click data is unchanged; trends will show a drop after visitors get the new collector, which areas and KPI comparisons should not read as an improvement in the product. The changelog and README say so.
