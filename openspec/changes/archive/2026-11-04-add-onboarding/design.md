# Design

## Context

- **First run today:** sign-up (`auth_api::signup`) and first setup (`auth_api::setup`) create an account with one website, and the dashboard (`assets/app.js`, `route()`) lands on Setup (`renderSetup`) until `project.collection.active`.
- **Setup:** one long page holding the install snippets (with a live poll that turns green on the first event), the `trace.json` instructions, custom-event examples, the MCP command, the analyst status and the "remove website" button. Once collecting, the Setup nav item hides and the header pill links back to it.
- **The facts each step needs are mostly stored already:**
  - collection status (`db::collection`);
  - KPIs (`kpis`, with `archived_at`);
  - custom privacy rules (`privacy_rules`);
  - tokens with `last_used_at` (`api_tokens`);
  - people and pending invites (`auth::user_count`, `auth::pending_invites`).
- **Site-scoped API routes** go through `auth_api::authorize`, which resolves the website from `/api/sites/{slug}/…` or `X-Trace-Site`, using `is_site_path` and the role table `required_role`.

## Goals / Non-Goals

**Goals:**
- One place that says what's left to set up, with each step's single action.
- No state to keep in sync: steps are derived from data Trace already has.

**Non-Goals:**
- Emails or reminders.
- Per-person progress: the checklist belongs to the website.
- Changing what Setup offers (it remains the reference page).

## Decisions

### Derive, don't store

Every step except privacy is computed on each request from existing tables. Storing "done" flags would drift, for example when a token is revoked or KPIs are removed. Privacy has no observable signal besides custom rules, so it gets `projects.privacy_reviewed_at` and `privacy_reviewed_by`, added by `db::migrate` like the other columns.

### "Agent connected" means a used token

A token that exists but was never used says nothing. `last_used_at` is set by MCP and API calls, so any used, unrevoked token in the account marks the step done. This is account-wide because tokens belong to people, not websites.

### Endpoints

- `GET /api/onboarding` returns `{ steps: [{ key, done, optional }], done, total, complete }`.
- `POST /api/onboarding/privacy-reviewed` records the confirmation and writes an `onboarding.privacy_reviewed` audit entry for the website.

Both join `is_site_path`. The POST requires admin, matching privacy rules.

### Dashboard

- **The `#/start` page:**
  - a step list on the left: a tick for done, a number for to-do, and the current step highlighted;
  - the current step's content on the right.
  - Steps reuse Setup's snippet builders, so there is one source for the snippets.
  - The install step polls `/api/project` while waiting, as Setup does today.
- **Navigation:** the "Get started" item shows `done/total` and hides when the checklist is complete. The default landing becomes `start` while the website has no data.
- **First runs:** sign-up and first setup redirect to `#/start`.
- **"Copy as a prompt for my agent":** builds a short instruction text containing the snippet. It is client-side only.

## Risks / Trade-offs

- **Two places show snippets** (Get started and Setup). Shared builders keep them identical.
- **A user can't finish step 3 alone:** confirming the privacy review needs an admin. The step says so for users.

## Migration Plan

`db::migrate` adds the two nullable columns. Existing websites start with privacy not reviewed unless they already have custom rules. That is correct: nobody has confirmed a review yet.

## Open Questions

None.
