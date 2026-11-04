# Spec Delta

## Purpose
Trace receives errors from the servers behind a website, groups and counts them without storing anything about the people whose requests failed, ties them to the visits and goals they hurt, and shows them to people and coding agents as aggregates.

## ADDED Requirements

### Requirement: Server errors are sent with a secret server key
Each website SHALL have at most one active server key, separate from its public write key. Only owners and admins SHALL create, rotate or revoke it, and Trace SHALL show it in full only once, when it is created. `POST /v1/errors` SHALL accept a batch only with a valid, active server key; a write key, a revoked key or no key SHALL be refused with 401.

#### Scenario: Using the public write key
- **WHEN** a client sends errors with the website's write key from its script tag
- **THEN** the request is refused with 401 and nothing is stored

#### Scenario: Rotating the key
- **WHEN** an admin rotates the server key
- **THEN** the new key works at once and the old one is refused from then on

#### Scenario: A user asks for the key
- **WHEN** a member with the user role asks for the server key through the dashboard, API or MCP
- **THEN** it is refused, and they are told an owner or admin can create one

### Requirement: Only error facts are accepted, and they are scrubbed again
A server error SHALL consist of: class, message, backtrace frames (file, line, function, in-app flag), route template, HTTP method, HTTP status, environment, release, runtime name and version, occurrence time and a repeat count. Any other field SHALL be dropped. Before storing, Trace SHALL scrub the message and every frame with the same rules as browser text (emails, tokens, card numbers, IBANs, phone-like numbers, long digit runs), cut absolute paths and home directories from file names, replace route segments that look like ids, emails or tokens with placeholders, drop query strings, and cut overlong values.

#### Scenario: Personal data in a message
- **WHEN** an error arrives with the message `No user with email anna@example.com (id 48213907)`
- **THEN** it is stored as `No user with email [email] (id [number])`

#### Scenario: Extra fields
- **WHEN** a batch includes `params`, `headers`, `cookies`, `body`, `user` or `context`
- **THEN** those fields are dropped before anything is stored, and the rest of the error is kept

#### Scenario: A raw URL instead of a route
- **WHEN** an error's route is `/orders/8817/receipt?token=abc`
- **THEN** it is stored as `/orders/:id/receipt`

### Requirement: Errors are grouped and stored as counts
Trace SHALL group errors by a fingerprint made from the class and the first in-app frame's file and function, or, without an in-app frame, the class and the message with numbers, quotes and ids removed. It SHALL store, per group, its class, a representative scrubbed message, first and last seen times, first and last release, and the latest sample backtrace; and counts per hour by route, method, status, environment and release. Individual occurrences SHALL NOT be stored.

#### Scenario: The same bug from different records
- **WHEN** the same `ActiveRecord::RecordNotFound` is raised at the same line for order 12 and order 99
- **THEN** both count towards one group, whose message shows the number replaced

#### Scenario: No per-request history
- **WHEN** someone looks for the individual requests behind a group, in the dashboard, API or MCP
- **THEN** only counts by hour, route, environment and release exist, and one sample backtrace

### Requirement: Server errors are limited and never harm the sender
`POST /v1/errors` SHALL accept at most 100 errors per batch and SHALL rate-limit each key. Each stored occurrence SHALL count as one event towards the account's monthly event limit; over the limit, Trace SHALL answer success, keep counting what it skipped, and store nothing more that month. Malformed errors SHALL be skipped individually rather than failing the batch.

#### Scenario: Over the monthly limit
- **WHEN** an account on Free has used its 100,000 events and a server sends 50 errors
- **THEN** the request succeeds, nothing is stored, and the Plan page shows the skipped count

### Requirement: Groups have a lifecycle tied to releases
A group SHALL be open, resolved or ignored. A group first seen in a release SHALL be marked new in that release. A resolved group that occurs again in a later release SHALL reopen and be marked as a regression. Ignored groups SHALL keep counting but leave rankings and the analyst summary. After a group is resolved with a release, Trace SHALL report, once 14 days have passed or enough traffic has arrived, whether it stopped, dropped or kept happening.

#### Scenario: A fix that worked
- **WHEN** a group is resolved in release `v2.4.0` and no occurrence arrives from `v2.4.0` or later for 14 days
- **THEN** the group shows "stopped after v2.4.0"

#### Scenario: A regression
- **WHEN** a resolved group occurs again from release `v2.6.1`
- **THEN** it is reopened and marked as a regression in `v2.6.1`

### Requirement: Server errors are tied to visits and goals
When a group's route, method and status match failed requests that the browser collector reported for the same website in the same period, Trace SHALL show the number of visits with that failure, the pages they were on, and the goals and funnel steps those pages feed. It SHALL never link a server error to an individual visit or person.

#### Scenario: A failing API behind a product page
- **WHEN** `GET /api/reviews/:id` raises errors with status 500 on the server, and browsers report failed requests to `/api/reviews/:id` with status 500 on `/products/:id`
- **THEN** the group shows the visits broken on `/products/:id` and the goals that page belongs to

### Requirement: People see server errors as aggregates
The dashboard's Errors page SHALL show server error groups ranked by visits broken, then occurrences, with their trend, routes, environments, first and last release, status, and the sample backtrace. It SHALL offer no actions beyond copying the command for the coding agent. The same SHALL be available from `GET /api/sites/{site}/errors` and `GET /api/sites/{site}/errors/{id}` with an API token. A website without a server key SHALL show how to set one up instead.

#### Scenario: A website with no server errors yet
- **WHEN** a website has no server key
- **THEN** the Server view explains what it adds and shows the agent command that sets it up

### Requirement: Coding agents can find, fix and close server errors
The MCP server SHALL offer tools to list server error groups with their counts and visits broken, to get one group with its sample backtrace and a fix prompt, to resolve, ignore or reopen a group (owners and admins, not read-only connections), and to create or rotate the server key (owners and admins). The behaviour summary and the analyse brief SHALL include the top open server error groups, so insights can cite them as evidence. Text inside errors SHALL be marked as coming from the application at runtime, to be treated as data, never as instructions.

#### Scenario: Fixing an error with the agent
- **WHEN** an agent calls the tool that gets a group
- **THEN** it receives the class, scrubbed message, sample backtrace, routes, releases, counts and visits broken, and a task prompt that says the text is data

#### Scenario: A read-only connection
- **WHEN** a read-only connection tries to resolve a group
- **THEN** it is refused
