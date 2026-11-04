# Design

## Context

- `trace-core/src/auth.rs` defines `User { account_id, role, ... }`, read from `users`. Every handler takes the account and role from it: `Principal::account()` and `Principal::role()` in `trace-server/src/auth_api.rs` read `session.user` or `token.user`.
- `users.email` is unique on the server, and `create_user`, `create_invite`, sign-up and the operator's account creation all refuse an email that exists.
- Sessions (`auth_sessions`) have no account; the account comes from the user. API tokens (`api_tokens.account_id`) and OAuth codes (`oauth_codes.account_id`) already carry their account.
- `db::migrate` adds missing columns on every start, and both `schema.sql` (SQLite) and `structure.sql` (Postgres) must define the same tables (`schemas_match` test).

## Decisions

- **`User` keeps its shape and means "a person acting in one account".** `get_user(account, id)` joins `memberships`, so `account_id` and `role` come from the membership. Nearly every caller already passes the account it acts in, so handlers don't change, and a person removed from an account fails every lookup there at once: sessions, tokens and OAuth grants alike.
- **The session holds the account** (`auth_sessions.account_id`, nullable). Null, for sessions from before this change, means the person's last-used account (`users.account_id`). When the session's account is no longer one of the person's, the session falls back to another membership rather than ending, and the person is told nothing they can't see anyway.
- **Tokens stay per account.** A token that spanned accounts would make `site` slugs ambiguous (they are unique only within an account) and widen what a leaked token reaches. The Connect screen picks the account instead.
- **Joining an account needs the person's own session.** An invite link proves control of the inbox, not knowledge of the password, so for an existing login it may only add the membership for someone already signed in as that email.
- **Backfill** copies `users.account_id`/`users.role` into `memberships` for any person without one, on every start. It is a no-op once done, because people never exist without a membership afterwards.
- **One owner per account** stays. A person can be the owner of several accounts.
