# Design

## Context

- `crates/trace-server/assets/d.js`: `start()` probed `localStorage`, created `trc_vid` and `trc_sid`, ended sessions after 30 idle minutes, and sent both ids in each batch. Sampling hashed `trc_vid`. Consent modes (`banner`, `required`) delayed `start()` until a yes.
- `crates/trace-core/src/ingest.rs`: `Batch.sid` was required; `store()` upserts the `sessions` row by that id.
- `crates/trace-server/src/api.rs`: `collect()` already resolves the client address (`guard::client_ip`, trusting `X-Forwarded-For` only from private peers) for rate limiting.

## Decisions

- **Hash, not store.** Visitor = SHA-256 of salt, website id, address and user-agent, cut to 32 hex characters and prefixed `v_`. Including the website keeps one visitor's hashes on two Trace sites unrelated.
- **Daily salts in the database**, not in memory, so restarts don't split sessions and several server processes agree. `INSERT ... ON CONFLICT DO NOTHING` makes the first request of the day race-free.
- **Yesterday's salt is kept** only to continue sessions across midnight; it is deleted the day after.
- **The collector still keeps a session id in memory**, to group batches the way it always has; it just doesn't send it.
- **Consent modes keep `localStorage`** after a yes, so sites that have consent get cross-day visitors.
- **Not done:** fingerprinting beyond address and user-agent. More signals would split shared networks better but would make the hash closer to a fingerprint.
