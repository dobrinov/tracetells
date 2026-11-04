# Spec Delta

## Purpose

Plans decide how much an account may use: how long its data is kept, how many websites and members it may have, how many events are recorded each month and how many Claude-powered analyses it may run. Plans come from templates and can be adjusted per account.

## ADDED Requirements

### Requirement: Plan templates and limits
A plan template SHALL define five limits:
- data retention in days
- websites per account
- events per month
- members per account
- Claude-powered analysis runs per month

An empty limit SHALL mean no limit. The server SHALL provide the templates `free`, `friend`, `owner` and `unlimited`, where `unlimited` has no limits. Changing a template SHALL change the effective limits of every account on it, except for limits that account overrides.

#### Scenario: The community default
- **WHEN** a server starts with no plan configuration and its first account is created
- **THEN** the account is on `unlimited`, and none of the five limits applies to it

#### Scenario: Editing a template
- **WHEN** the operator changes `free` retention from 7 to 3 days
- **THEN** every account on `free` without a retention override keeps 3 days of raw events from then on

### Requirement: Default plan for new accounts
New accounts SHALL be put on the server's default plan, which SHALL be `unlimited` unless the operator configures another template as the default.

#### Scenario: A hosted server
- **WHEN** the server's default plan is `free` and someone signs up
- **THEN** their new account is on `free`

### Requirement: Per-account overrides
The operator SHALL be able to override any of the five limits for one account. An override SHALL apply to that account only, take precedence over the template, and be removable so the template's value applies again.

#### Scenario: More retention for one free account
- **WHEN** an account is on `free` with 7 days' retention and the operator sets a 30-day retention override for it
- **THEN** that account keeps 30 days of raw events while other `free` accounts keep 7

### Requirement: Retention limit
Raw events older than the account's effective retention SHALL be deleted. The server-wide retention setting SHALL still apply when it is shorter. Aggregated results (ideas, KPIs, evaluations) SHALL be kept.

#### Scenario: A short-retention account
- **WHEN** an account's effective retention is 7 days and the server's setting is 90 days
- **THEN** its raw events older than 7 days are deleted, while an `unlimited` account on the same server keeps 90 days

### Requirement: Website and member limits
Adding a website, or inviting a person, beyond the account's limit SHALL be refused with a message stating the limit. Pending invites SHALL count as members. Lowering a limit SHALL never delete websites or people.

#### Scenario: Lowering the website limit
- **WHEN** an account has three websites and its website limit is changed to one
- **THEN** all three websites keep working, and adding a fourth is refused

#### Scenario: Inviting at the member limit
- **WHEN** an account allows 3 members and has 2 people and 1 pending invite
- **THEN** a new invite is refused with a message stating the limit

### Requirement: Monthly event limit
Events above the account's monthly limit SHALL be dropped and counted until the next calendar month (UTC), and the collector SHALL still receive a success response so visitors' browsers do no extra work.

#### Scenario: Monthly events exhausted
- **WHEN** an account on a 100,000-events plan has recorded 100,000 events this month and a visitor's browser sends more
- **THEN** the collector gets a success response, the events are not stored, and the dashboard shows how many events were not recorded

### Requirement: Analysis run limit
Once Claude-powered analysis runs reach the account's monthly limit, automatic analysis SHALL use the built-in heuristics and say so, and a request that explicitly asks for Claude SHALL be refused. Heuristic analysis SHALL never be limited.

#### Scenario: Analysis limit reached
- **WHEN** an account has used all its Claude-powered analysis runs this month and someone runs analysis
- **THEN** the run uses the built-in heuristics and the result says the monthly Claude limit was reached

### Requirement: Customers see limits, not plan names
Plan template names SHALL be visible only to operators. People in an account SHALL be able to see their account's effective limits and this month's usage of each, and SHALL be told how to ask for more.

#### Scenario: A person on the friend plan
- **WHEN** a person on an account on `friend` opens Plan & usage
- **THEN** they see their limits and usage, for example "Data kept for 365 days" and "2 of 5 websites", but not the word "friend"
