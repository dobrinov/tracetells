# Proposal

This is a feature.

## Why

The operator area was one page inside the dashboard. The owner wants a separate interface showing accounts, people, websites and usage across the server, and to be able to see the dashboard as one of the people using it.

## What Changes

- **`/admin`**, its own page (same styles and content security policy as the dashboard): Overview (`GET /api/operator/stats`: accounts, people, websites, sessions, events, sessions per day), Accounts (as before), People (`GET /api/operator/people`), Websites (`GET /api/operator/sites`) and Plans. The dashboard's Admin link opens it; the old in-dashboard page is gone.
- **Viewing as someone** (`POST /api/operator/impersonate`, `POST /api/auth/stop-impersonating`): an operator opens the dashboard as a non-operator, read-only (GET, `/query` and `/account/switch` only; no operator area), for at most an hour, without touching that person's last-used account or sign-in. A banner shows on every page with a way to stop. Recorded in the operator's log and, as `operator.viewed_as`, in the viewed account's log. `auth_sessions.impersonator_id` marks such sessions.

## Capabilities

### New Capabilities
- `admin-interface`: the operators' view of the whole server, and viewing as a person.

### Modified Capabilities
None.

## Privacy impact

Operators could already see account metadata, people, websites and usage. Viewing as someone also shows them that account's analytics, which they could not see before: it is read-only, limited to an hour, never available for another operator, and visible to the customer in their own activity log.

## Site impact

None: the collector is unchanged.
