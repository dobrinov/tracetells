# Proposal

This is a feature.

## Why

Connecting a coding agent meant creating a token in the dashboard, pasting it into a command, and then doing setup (snippet, KPIs, privacy) in the dashboard anyway. The owner wants to control everything over MCP: connect by signing in, and let the agent set the website up from the codebase.

## What Changes

- **Sign in through MCP.** This is the MCP authorization flow: OAuth 2.1, the authorization-code grant with PKCE (S256), and dynamic client registration.
  - `/mcp` without a token answers 401, with `WWW-Authenticate: Bearer resource_metadata=…`.
  - Discovery: `/.well-known/oauth-protected-resource` and `/.well-known/oauth-authorization-server`.
  - New endpoints: `/oauth/register`, `/oauth/authorize` and `/oauth/token`.
  - Authorizing hands over to a new dashboard screen, `#/connect`. There the person signs in (with two-step verification when on) and approves, optionally read-only.
  - The code is single-use, expires after 10 minutes, and is bound to the client, the redirect address and the PKCE challenge. It is traded for a normal API token named after the app, listed and revocable under Account.
  - Redirect addresses must be HTTPS, loopback HTTP, or an app scheme.
- **Setup over MCP:**
  - `add_site` (admins), `get_install_snippet` (the tag plus where it goes, single-page apps, custom events, consent and masking) and `get_setup_status`;
  - `push_config` (admins; applies `trace.json`), `add_privacy_rule` and `remove_privacy_rule` (admins; the running server reloads its collectors);
  - `confirm_privacy_review` (only on the person's word);
  - `get_privacy_rules` now shows rule ids.
  - The server's instructions describe the setup steps.
- **The setup checklist** puts "Connect your coding agent" second, since the agent can do the next steps.
- **The dashboard copy** tells people to `claude mcp add` the URL and authenticate (no token), and offers a ready setup prompt. Tokens remain for CI and scripts.

## Capabilities

### New Capabilities
- `mcp-sign-in`: connecting an MCP client by signing in and approving it.
- `mcp-setup`: setting a website up through MCP tools.

### Modified Capabilities
None.

## Privacy impact

None for visitors. Registered clients store their name and redirect addresses. Codes are stored hashed and deleted when used or expired.

## Site impact

None: the collector is unchanged.

## Impact

- `trace-core`: `oauth.rs`, and `oauth_clients`/`oauth_codes` in both schemas; `onboarding.rs` (order).
- `trace-server`: `oauth.rs`, `api.rs` (routes, 401 challenge, `public_url`), `auth_api.rs`, `mcp.rs` (setup tools, `origin` and `state` in the context), `main.rs`, and the dashboard (Connect screen, copy).
- README: the MCP section.
- Policy change: an admin's MCP token can now add websites and apply `trace.json`. It still can't remove websites or manage people and tokens.
