# Tasks

## 1. Limits and addresses

- [x] 1.1 Add `guard::Counter`, an expiring, size-bounded counter, and `guard::client_ip`, which believes `X-Forwarded-For` only from private addresses
- [x] 1.2 Limit collector batches per address and per website; give back quota for rejected batches
- [x] 1.3 Limit sign-in per email and address, per email and per address; sign-ups per address; two-step codes per person

## 2. Accounts and passwords

- [x] 2.1 Run Argon2 hashing and checking on the blocking pool
- [x] 2.2 Only the owner can act on the owner

## 3. Headers

- [x] 3.1 Add security headers, with a dashboard Content-Security-Policy that allows the inline theme script by hash; check in a browser that nothing is blocked

## 4. Tests and deployment

- [x] 4.1 Tests: counter expiry and bound, forwarded addresses, CSP hash, owner protection, sign-in limits, collector flood, headers
- [x] 4.2 `Dockerfile`, `.dockerignore`, `deploy/` (compose, Caddy site, deploy script, README) and README links
