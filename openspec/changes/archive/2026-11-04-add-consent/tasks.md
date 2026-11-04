# Tasks

## 1. Collector

- [x] 1.1 Add the `consent` option (`banner`, `required`), `consent-text` and `consent-link`; gate `start()` on a stored yes; queue calls until then
- [x] 1.2 `trace.consent(yes)` and `trace.consent()`, including calls queued through the stub; No thanks and withdrawal delete the ids and stop storing and sending; Global Privacy Control counts as no
- [x] 1.3 The bar: one sentence, an optional "How it works" link, Allow and No thanks styled alike, real buttons; `data-trace-consent` reopens it
- [x] 1.4 Check in a browser: nothing stored or sent before an answer; Allow starts collection and is remembered; withdrawing deletes the ids and stops sending; Global Privacy Control shows no bar

## 2. Dashboard and docs

- [x] 2.1 "Ask visitors for consent first" on the Setup and Get started install snippets (on by default), and in the agent install prompt
- [x] 2.2 README: the options, the API and an "Asking for consent" section
