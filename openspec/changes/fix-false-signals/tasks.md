# Tasks

## 1. Collector: dead clicks

- [x] 1.1 Add a passive capturing `pointerdown` listener in `d.js` that starts the watch for one second and keeps the press baseline (time, mutation count, scrolls, address)
- [x] 1.2 The click handler uses a press from the last second as its baseline, and the click itself otherwise (keyboard, synthetic clicks)
- [x] 1.3 Exclude `<canvas>` from the dead-click check, keyed on the element clicked
- [x] 1.4 Exclude clicks whose judged element is `html` or `body`
- [x] 1.5 In the one-second check, skip when `getSelection()` holds a non-empty selection; never read the selection's text into an event
- [x] 1.6 Keep the collector ES5 and passive; `the_collector_stays_small` passes, with the budget and every published size claim raised to 8.4 KB brotli / 9.5 KB gzip (the fix is about 70 bytes over the old budget after trimming)

## 2. Collector: rage clicks

- [x] 2.1 Record a rage click in the one-second check of the burst's third click, only if nothing changed since the burst's first press or click
- [x] 2.2 No rage click where no dead-click check runs (form fields, labels, links that open elsewhere, `html`/`body`, canvas, no MutationObserver) or when text is left selected
- [x] 2.3 A broken button still gives one dead click and one rage click per burst; repeated dead clicks on one element still count once within 2 s

## 3. Matchers

- [x] 3.1 In `Parser::matcher` (`crates/trace-core/src/config.rs`), refuse a page pattern that isn't `*` and doesn't start with `/`, from the shorthand or `"path"`, with the `"text"` hint for error and click types
- [x] 3.2 Refuse unknown keys: in the `type` form anything but `type`/`path`/`selector`/`name`/`text`; in the shorthand form name the extra key(s) instead of only "expected one event type"; suggest `"text"` for `message`
- [x] 3.3 Tests: `{"error": "Missing target*"}` refused with the hint, by `parse_matcher` and by `check_config`; `{"type": "error", "message": "X"}` and `{"error": "*", "name": "X"}` refused naming the key; `{"error": "/checkout"}`, `{"pageview": "/products/*"}` and `{"error": "*", "text": "Unhandled rejection: Missing*"}` accepted and matching on SQLite and Postgres; `{"type": "error", "text": "no-such*"}` counts 0
- [x] 3.4 `query_behaviour` returns the refusals as tool errors; a test in `mcp.rs`
- [x] 3.5 The `query_behaviour` description in `mcp.rs` says `"text"` matches an error's whole message, and that unhandled rejections start with `Unhandled rejection: `

## 4. Checking in a browser

- [x] 4.1 A local test page with: a button that changes on `pointerdown` only, a canvas that draws, a number pad updating a readout, a button that does nothing, a paragraph, an input and empty margins. Load `d.js` in it, with the events it sends captured in the page, and drive it with real mouse input
- [x] 4.2 Only the do-nothing button gives dead clicks, and only clicking it three times quickly gives a rage click; clicking the margin, selecting the paragraph and triple-clicking the input give neither. The same run against the previous collector gives dead or rage clicks on every element, confirming the page reproduces the bugs

## 5. Docs and release

- [x] 5.1 README note (there is no changelog file): dead and rage clicks now count only presses the page ignored, not the page itself, text selection or drawing, so their numbers drop once visitors get the new collector; trace.json matchers with a page pattern not starting with `/` or an unknown key are now refused
- [x] 5.2 The dashboard's Behaviour help text says the same
- [ ] 5.3 Before deploying, list the hosted server's stored KPIs and funnel steps whose matchers the new checks refuse, and tell those accounts (or fix our own sites' trace.json)
- [ ] 5.4 Deploy the server: this ships the collector fix, and also text matching (`ae4f9d5`), `get_defects` and the current `query_behaviour` description, which the hosted server lacks. Afterwards, `{"type": "error", "text": "zzz*"}` on wunderkind.bg answers 0
