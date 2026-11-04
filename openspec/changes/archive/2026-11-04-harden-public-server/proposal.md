# Proposal

This is a security change, needed before the hosted server is reachable from the internet.

## Why

The CTO review found that a public server could be abused: anyone can send unlimited collector batches (using up an account's monthly quota and skewing its data), sign-in limits were per email only (password spraying, lockouts), password hashing ran on the async workers (a burst of sign-ins stalls the server), an admin could reset the owner's two-step verification, and no security headers were sent. Behind a reverse proxy, per-address limits also saw only the proxy's address.

## What Changes

- Collector: at most 600 batches a minute per visitor address and 6,000 per website, refused with 429 before parsing; a batch that ingest rejects no longer uses quota.
- Sign-in: limits per email and address (10), per email from anywhere (50) and per address across emails (30), each for 15 minutes; sign-ups per address (10); two-step codes per person (10). The limiter's memory is bounded and expires.
- The client's address is read from `X-Forwarded-For` only when the connection comes from a private address (the reverse proxy).
- Argon2 runs on the blocking thread pool.
- Only the owner can act on the owner (reset two-step, sign out, change access).
- Security headers on every response: `X-Content-Type-Options`, and except for the collector `X-Frame-Options: DENY`, `Referrer-Policy: same-origin`, `Cross-Origin-Opener-Policy`, and a Content-Security-Policy for the dashboard that allows its one inline script by hash.
- Deployment: a `Dockerfile` and `deploy/` (compose, Caddy site, deploy script, README) for a host with a shared Caddy and Postgres.

## Capabilities

### New Capabilities
- `server-protection`: rate limits, client addresses behind a proxy, role protection for the owner, and security headers.

### Modified Capabilities
None.

## Privacy impact

None collected or stored. Visitor and sign-in addresses are held in memory only, for up to 15 minutes, to count attempts; they are never written to the database or logs.

## Site impact

None for the collector script. A site sending more than 600 batches a minute from one address, or 6,000 for one website, gets 429 for the rest of that minute; the collector already ignores failed sends.

## Impact

`crates/trace-server/src/guard.rs` (new), `api.rs` (collect, headers), `auth_api.rs` (limits, owner protection), `crates/trace-core/src/auth.rs` (hashing off the async workers), `Dockerfile`, `.dockerignore`, `deploy/`, README.
