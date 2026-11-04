# Proposal

This is a feature.

## Why

A person belongs to exactly one account today: `users.account_id` and `users.role` hold it, and an email can't be invited anywhere once it exists. But people work across accounts. The owner invited a colleague into his account, and that colleague also wants an account of his own with his own websites. Today the colleague would need a second email address for that, and the invite would fail if he signed up first.

## What Changes

- **Memberships.** A new `memberships` table holds one row per person per account, with the role in that account. A person can own one account, be an admin in another and a user in a third. Existing people get one membership each, for the account and role they have now.
- **Signing in** still asks only for email and password. It opens the account used last.
- **Switching accounts.** A signed-in person can switch to any account they belong to from the website menu (`POST /api/account/switch`). The session remembers the account. Each account's own rules still apply on switching, such as required two-step verification.
- **Invites work for people who already have a login.** An admin can invite any email that isn't already a member. A person who already has a login joins by signing in as that email and pressing Join; the invite link never sets or changes their password. A link for another email is refused.
- **A second account.** When sign-up is open, a signed-in person can create a new account they own (`POST /api/account/new`) without a second email. The operator's "create account" can now invite someone who already has a login as its owner.
- **Leaving.** A person can leave an account they don't own (`POST /api/account/leave`). Removing someone from an account removes only that membership and revokes their tokens for that account; their login and other accounts stay. A person left with no account at all is deleted as before.
- **Tokens and MCP.** An API token still belongs to one account. Connecting an MCP client asks which account to connect when the person has several.
- **CLI.** `trace user create` adds an existing person to the named account instead of failing. `trace user list` shows one line per membership.

## Capabilities

### New Capabilities
- `account-membership`: one login in several accounts, switching between them, joining by invite, creating and leaving accounts.

### Modified Capabilities
None.

## Privacy impact

None for visitors. For people using the dashboard: an invite link now tells whoever holds it whether its email already has a login on the server, which the invitee needs in order to sign in rather than choose a password. The link is a secret sent to that email.

## Site impact

None: the collector is unchanged.

## Impact

- `trace-core`: `auth.rs` (memberships behind `User`, which keeps meaning "a person in an account"), `db.rs` (migration and backfill), `onboarding.rs`, both schemas.
- `trace-server`: `auth_api.rs` (switch, new, leave, joining by invite, accounts in the auth state), `oauth.rs` (choosing the account), `operator.rs`, `main.rs` (CLI), `demo.rs`, and the dashboard (account switcher, join screen, Account page).
- `users.account_id` now means the account sign-in opens; `users.role` is no longer read and stays only because SQLite can't drop a NOT NULL column cheaply.
- README: the team section.
