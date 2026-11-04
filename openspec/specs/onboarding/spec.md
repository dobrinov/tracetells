# onboarding Specification

## Purpose
A per-website setup checklist that starts by connecting a coding agent and then follows outcomes (data arriving, goals defined, a first insight), ticking each step off from what Trace observes.

## Requirements

### Requirement: Setup checklist per website
Trace SHALL offer each website a checklist of seven steps in this order: agent, data, goals, insight, privacy, team and server errors. The first four are required and the last three optional. Each step SHALL report whether it is done.

#### Scenario: A brand-new website
- **WHEN** an account has just created a website that has received no events, has no KPIs, insights or custom privacy rules, and the account has one person and no used API token
- **THEN** no step is done

#### Scenario: Steps follow what Trace observes
- **WHEN** an API token of the account is used, the website receives its first event, a KPI is pushed for it in `trace.json`, and an insight is saved
- **THEN** the agent, data, goals and insight steps show as done without anyone marking them, and the checklist is complete

### Requirement: Confirming the privacy review
Admins SHALL be able to confirm they have reviewed what a website protects. The privacy step, which is optional, SHALL be done once that is confirmed, or once the website has at least one custom privacy rule. The confirmation SHALL be recorded in the account's activity log.

#### Scenario: An admin confirms the review
- **WHEN** an admin confirms the privacy review for a website
- **THEN** that website's privacy step is done, other websites' privacy steps are unchanged, and the activity log shows who confirmed it

#### Scenario: A user can't confirm
- **WHEN** a person with the user role tries to confirm the privacy review
- **THEN** the request is refused and the step stays not done

### Requirement: Optional steps never block
The team step SHALL be done when the account has more than one person or a pending invite, and the server errors step once the website has received a server error. Leaving the privacy, team or server errors step undone SHALL NOT keep the checklist from being complete.

#### Scenario: A solo founder finishes setup
- **WHEN** the four required steps are done, the account has one person and no invites, and no server errors were sent
- **THEN** the checklist is complete

### Requirement: Where the checklist shows
The dashboard SHALL show "Get started" under Setup until the checklist is complete. It SHALL open on the checklist when a new account is created; otherwise it SHALL open on the website's Home. Once complete, the item SHALL disappear from the navigation.

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
