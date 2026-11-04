# Spec Delta

## Purpose

A per-website setup checklist that walks a new account through installing the collector, reviewing privacy, defining KPIs, connecting a coding agent and inviting a team, ticking each step off from what Trace observes.

## ADDED Requirements

### Requirement: Setup checklist per website
Trace SHALL offer each website a checklist of six steps in this order: website, install, privacy, KPIs, agent and team. The first five are required and the last is optional. Each step SHALL report whether it is done.

#### Scenario: A brand-new website
- **WHEN** an account has just created a website that has received no events, has no KPIs and no custom privacy rules, and the account has one person and no used API token
- **THEN** the checklist shows the website step done and the install, privacy, KPIs, agent and team steps not done

#### Scenario: Steps follow what Trace observes
- **WHEN** the website receives its first event, a KPI is pushed for it in `trace.json`, and an API token of the account is used
- **THEN** the install, KPIs and agent steps show as done without anyone marking them

### Requirement: Confirming the privacy review
Admins SHALL be able to confirm they have reviewed what a website protects. The privacy step SHALL be done once that is confirmed, or once the website has at least one custom privacy rule. The confirmation SHALL be recorded in the account's activity log.

#### Scenario: An admin confirms the review
- **WHEN** an admin confirms the privacy review for a website
- **THEN** that website's privacy step is done, other websites' privacy steps are unchanged, and the activity log shows who confirmed it

#### Scenario: A user can't confirm
- **WHEN** a person with the user role tries to confirm the privacy review
- **THEN** the request is refused and the step stays not done

### Requirement: The team step is optional
The team step SHALL be done when the account has more than one person or a pending invite. Leaving it undone SHALL NOT keep the checklist from being complete.

#### Scenario: A solo founder finishes setup
- **WHEN** the five required steps are done and the account has one person and no invites
- **THEN** the checklist is complete

### Requirement: Where the checklist shows
The dashboard SHALL show a "Get started" navigation item with the number of required steps done, until the checklist is complete. It SHALL open on the checklist for new accounts and for websites that have received no events. Once complete, the item SHALL disappear from the navigation, and the checklist SHALL stay reachable from the Setup page.

#### Scenario: Signing up
- **WHEN** someone creates an account
- **THEN** the dashboard opens on the checklist for the account's first website

#### Scenario: Finishing
- **WHEN** the last required step of the current website becomes done
- **THEN** the "Get started" item is no longer in the navigation

### Requirement: Scoped like everything else about a website
The checklist SHALL be scoped to one website of the caller's account. Another account's website SHALL be reported as not found. The checklist SHALL reveal only done/not-done states, never individual people or sessions.

#### Scenario: Another account's website
- **WHEN** a person asks for the checklist of a website in a different account
- **THEN** the response is "not found"
