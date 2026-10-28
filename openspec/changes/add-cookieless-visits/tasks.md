# Tasks

- [x] 1 `trace_core::visits`: daily salts, visitor hash, session continuation across midnight, sampling position; `daily_salts` in both schemas; tests
- [x] 2 Collect endpoint: derive the visit for batches without a session id; sample by visitor before quota; end-to-end test
- [x] 3 Collector: no storage without consent; no ids in cookieless batches; `sample` sent to the server; checked in a browser on the demo shop
- [x] 4 Dashboard and MCP snippets cookieless by default, with "Wait for my consent tool"
- [x] 5 README, homepage and privacy policy
- [x] 6 Visitor total: the identify() id where known, else the daily hash; test
