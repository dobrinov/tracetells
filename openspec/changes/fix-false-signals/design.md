# Design

## Context

**Clicks in the collector** (`crates/trace-server/assets/d.js`, "clicks: plain, rage and dead"). One capturing, passive `click` listener does everything:
- **Rage** is decided at once. `burst` keeps clicks from the last 700 ms within 40 px, and the third one pushes `rage_click` in the idle callback, whatever the page did.
- **Dead** is decided a second later. At click time `watch()` starts a shared MutationObserver (reference-counted by `watching`), and the handler notes `mutations`, `scrolls` and `location.href`. One second after the click it checks whether any of them changed. `FORM_FIELD` (`INPUT|SELECT|TEXTAREA|LABEL|OPTION`) and links that open elsewhere are excluded.
- **What slips through:**
  - A handler that changes the page on `pointerdown` makes its change before the observer starts.
  - A `<canvas>` changes no DOM at all.
  - A number pad updates its readout on every click, but rage ignores that.
  - The judged element is `raw.closest(INTERACTIVE) || raw`, so a click on empty space is judged as `body` or `html`, and a click that selects text in a paragraph is judged as the `p`. Neither changes the DOM.
  - Rage is pushed before `deadCheck` is consulted, so a triple-click that selects an input's contents or a line of text is a rage click, even on form fields.

**Who reads dead clicks.** The defect sweep (`get_defects`) already looks only at dead clicks on buttons and links. Areas' "with problems" share and `query_behaviour` count all of them, which is where the false ones did their damage.

There are no JavaScript tests. The collector is checked by `the_collector_stays_small` and the strip tests in `crates/trace-server/src/api.rs`, and by hand in a browser.

**Matchers** (`crates/trace-core/src/config.rs`, `Parser::matcher`, also behind `parse_matcher` for `query_behaviour`):
- The shorthand value is the page for every type except `custom` (name) and the click types (selector). `"text"` is matched against the stored text, for errors their message, in `query.rs` (SQL) and `model.rs` (in memory), with a trailing `*` as prefix.
- Nothing checks that a page pattern looks like a page, so `{"error": "Missing target*"}` parses and matches nothing.
- In the `type` form, keys other than `type`, `path`, `selector`, `name` and `text` are silently dropped. In the shorthand form, any key other than `path`, `selector` and `text` counts as a second event type and gets "expected one event type", which doesn't name it.
- The repo's `query_behaviour` description already shows `{"error": "*", "text": "POST /api/cart*"}`. The hosted server predates text matching (`ae4f9d5`): checked on 2026-11-04, it refuses that form and ignores `"text"` in the `type` form, counting every error. This is part of why the analysis missed it.

## Goals / Non-Goals

**Goals:**
- Dead and rage clicks mean the page didn't react, including to presses handled on `pointerdown` and to drawing.
- Clicks on the page itself and clicks that select text aren't judged at all.
- A matcher that can never match, or that has a key it would ignore, is refused with a hint, wherever matchers are parsed.
- The hosted server runs this code, so `"text"` works there.

**Non-Goals:**
- Rewriting stored rage and dead clicks: past data stays as recorded.
- Detecting reactions that change neither the DOM nor scroll, other than canvas (WebGL, audio, `<video>` controls). Sites can still mark such elements, see Risks.
- Judging only elements that look interactive. That would also drop controls whose handler is attached in script (wunderkind's `li.choice-button` is one), which are real dead-click candidates. Clicks on `main`, `section` or a `p` without a selection stay judged; areas and the analysis can weigh them by selector.

## Decisions

**Watch from the press, not the click.**
- A passive capturing `pointerdown` listener calls `watch()` and stores the press time and the page's state (mutation count, scrolls and address, as one string) as `press`.
- The click handler uses `press` as its baseline when it arrived within 1 s; otherwise (keyboard clicks, synthetic clicks) it takes the baseline at click time, as today.
- `press` holds its own `watch()` reference for exactly one second. A click that uses it comes within that second and starts its own watch, so the observer runs without a gap from the press to the click's check. A press with no click (a drag that ends elsewhere) costs one second of watching. A fixed timer keeps the count balanced with no bookkeeping, which matters for the size budget.
- *Alternative considered:* checking for mutations "around" the click with timestamps from the observer records. That would need the observer always on, which goes against "pays nothing between clicks".

**Skip canvas like form fields.** Add `CANVAS` to the elements excluded from the dead-click check, keyed on the element clicked rather than its interactive ancestor. Excluding `touch-action: none` was rejected: it would need `getComputedStyle`, a layout-adjacent read on the hot path.

**Skip the page itself and selected text.**
- Not judged: a click whose judged element (`closest(INTERACTIVE) || raw`) is `HTML` or `BODY`. Keyed on the judged element, so a button inside `body` is still judged.
- Not judged: a click where, in the one-second check, `getSelection()` is not collapsed. Only `isCollapsed` is read, not the selection's text, which would also mean serialising it. Read in the check, not the handler, so it costs nothing on the hot path. The selection's text is never read into an event, only whether there is one.
- *Alternative considered:* testing the selection at click time. A drag-select is finished by then, but a triple-click's selection grows with each click; reading it once, a second later, covers both.

**Rage waits for the dead-click verdict.**
- Keep the cheap `burst` arithmetic. When a burst reaches three, store the baseline of the burst's first click (its press, or the click itself) on the third click's pending check, instead of pushing `rage_click` at once.
- In the one-second check, push `rage_click` only if nothing changed since that baseline.
- No rage click where no dead-click check runs: form fields and labels, links that open elsewhere, `html`/`body`, canvas, browsers without MutationObserver, and when text is left selected. A rage click is "the page ignored a burst", and these are the cases where Trace can't tell, or where the burst was selecting text.
- The dead click for the third click is then pushed or not as today, so a broken button still gives one dead and one rage click.
- The cost: a rage click arrives a second later, and one is lost if the page unloads within that second. That's acceptable for an aggregate signal.

**Refuse page patterns that aren't pages.**
- In `Parser::matcher`, after the matcher is built, check `path`: it must be `None` (meaning `*`) or start with `/`, wherever it came from (shorthand or `"path"`).
- The error message says page patterns start with `/`. For `error`, `click`, `rage_click` and `dead_click` it adds: put the message or label in `"text"`, e.g. `{"error": "*", "text": "<value>"}`.
- Because `check_config`, `push_config`, `query_behaviour`, `draft_insights` evidence and funnel steps all go through this parser, they all refuse it the same way.
- *Alternative considered:* silently treating a non-`/` error value as text. Rejected: it guesses, and makes `{"error": X}` mean two things.

**Refuse keys a matcher would ignore.**
- In the `type` form, any key other than `type`, `path`, `selector`, `name` and `text` is an error naming the key and listing the allowed ones.
- In the shorthand form, when there's more than one candidate type key, say which keys were seen. For a known non-type key such as `name` or `message`, say what to use instead (`{"event": …}` for a custom event's name, `"text"` for an error's message).
- An error rather than a warning, because an ignored key changes the answer: it widens the match to everything.

**Description.** In `mcp.rs`, the `query_behaviour` description says that `"text"` matches an error's whole message, and that unhandled rejections are stored as `Unhandled rejection: …`.

## Risks / Trade-offs

- **[A trace.json that used a non-`/` page pattern, or an unknown matcher key, now fails `check_config` and `push_config`]**: the first never matched anything. The second matched more than meant, so fixing it changes that KPI's numbers, which is the point. The error names the matcher and the fix. Before deploying, check the hosted server's stored KPIs and funnels for either, so no account's config stops applying unannounced.
- **[Watching from the press counts more as a reaction]**: a script that adds a class on mousedown or focus (some focus-ring and ripple libraries) now counts as the page reacting, hiding a real dead click on that control. Accepted: wrongly calling a working control dead is the worse error for insights.
- **[A visitor who selects text and also hit a dead control]** within the same second isn't counted. That's rare, and a broken control is clicked again.
- **[Trends drop when the new collector reaches visitors]**: areas and KPIs built on rage and dead clicks will improve without any product change. The changelog says so, and so does a line in the dashboard's Behaviour help text.
- **[Reactions outside the DOM are still judged dead]**: for example audio, WebGL or a CSS-only `:active` state. These are rare and stay as before.
- **[A page that reacts within the second for unrelated reasons]**, such as a ticking clock, hides dead and rage clicks: this was already true of dead clicks.
