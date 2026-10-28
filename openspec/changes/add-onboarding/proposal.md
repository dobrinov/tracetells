# Proposal

A feature: a guided first run, so a new account understands and sets up everything in one go.

## Why

Today a new account lands on Setup: one long page of snippets, KPI instructions and MCP commands, with no sense of what is done and what is left. Nothing tells you whether you have reviewed what is protected, defined a KPI or connected an agent. The design ("Onboarding: first website" on the Trace design canvas) replaces that with a short checklist. Each step explains one thing, shows the one action it needs, and ticks itself off when Trace can see it's done.

## What Changes

- A **Get started** page per website, with six steps:
  1. **Your website**: its name, and how to add another.
  2. **Install the collector**: the snippet per kind of site, a "copy as a prompt for my agent" button, and a live status that turns green when the first event arrives.
  3. **Check what's protected**: the built-in protections and the website's own rules. Done once someone confirms they've reviewed them, or adds a rule.
  4. **Define KPIs in code**: an example `trace.json` and the `check` / `push` commands. Done when the website has a KPI.
  5. **Connect your coding agent**: the MCP command and a link to create a token. Done once any API token in the account has been used.
  6. **Invite your team** (optional): done when the account has more than one person or a pending invite.
- Steps tick themselves off from what Trace can observe. The only stored state is "privacy reviewed", per website.
- The navigation shows **Get started** with its progress (e.g. "3 of 5") until the required steps are done, then hides it. The page stays reachable from Setup.
- New accounts (sign-up and first setup) land on Get started. A website with no data yet also lands there.
- Copy that names the steps uses the product name Trace and the term "insights".

## Capabilities

### New Capabilities

- `onboarding`: the per-website setup checklist, its step states, and when it shows.

### Modified Capabilities

None.

## Privacy impact

None about visitors. One new value is stored per website: when someone confirmed the privacy review, and who. The "agent connected" step reads whether a token has been used, which Trace already records. Nothing new is collected from visitors or exposed through the API beyond done/not-done flags.

## Site impact

None: the collector is unchanged.

## Impact

- `crates/trace-core`: a `privacy_reviewed_at` / `privacy_reviewed_by` column on `projects` with its migration, and a function computing the step states.
- `crates/trace-server`: `GET /api/onboarding` and `POST /api/onboarding/privacy-reviewed` (site-scoped; reading needs the user role, confirming needs admin, like privacy rules), with an activity log entry.
- Dashboard: the Get started page and nav item, plus the landing rules.
- README: a short "First run" section.
