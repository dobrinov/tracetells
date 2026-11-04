# Spec Delta

## Purpose

Let people and their agents ask aggregated questions about how visitors behave on a website, and get numbers back.

## ADDED Requirements

### Requirement: Matchers and measures
A matcher SHALL name an event type (`pageview`, `click`, `rage_click`, `dead_click`, `error`, `custom`, `form_start`, `submit`, `scroll` or `leave`), optionally with a path, selector or event name; a pattern ending in `*` SHALL match by prefix. The measures SHALL be `sessions`, `events`, `session_rate`, `per_session`, and `conversion` from one matcher to another.

#### Scenario: A prefix
- **WHEN** an agent asks for the `sessions` with a `pageview` at `/docs/*`
- **THEN** sessions viewing any page under /docs count

### Requirement: Aggregated questions
Trace SHALL answer a query over a website's collected events, made of a matcher, a measure and a window of 1 to 90 days. Each answer SHALL include the value, the number of sessions it covers, and the window's start and end.

#### Scenario: A rate
- **WHEN** an agent asks for the `session_rate` of `rage_click` on selector `button#pay` with path `/checkout`, over 14 days
- **THEN** Trace answers with the share of sessions in those 14 days that had such a rage click, the number of sessions it was measured over, and the window

#### Scenario: A funnel
- **WHEN** an agent asks for the `conversion` from `pageview /checkout` to `custom purchase`
- **THEN** Trace answers with the share of sessions that viewed /checkout and later sent `purchase`, and how many sessions viewed /checkout

### Requirement: Breakdowns and comparisons
A query SHALL accept a breakdown by `path`, `selector`, `device` or `day`, and SHALL return one row per value, largest first, with at most 50 rows. A breakdown value covering fewer than 3 sessions SHALL be merged into a row named "other". A query SHALL accept `compare: true`, which adds the same measure over the window just before it.

#### Scenario: By device
- **WHEN** an agent breaks the Pay rage-click rate down by device
- **THEN** each device appears with its own rate and session count

#### Scenario: A rare page
- **WHEN** a breakdown by path includes a page seen in a single session
- **THEN** that page doesn't appear by name; its numbers are part of "other"

### Requirement: Aggregates only
Queries SHALL never return session ids, visitor ids, user ids, user agents, timestamps of single events, or single events. Text in queries SHALL be validated (length, allowed characters), and invalid queries SHALL be refused with a message saying what to fix.

#### Scenario: Asking for people
- **WHEN** an agent asks for a breakdown by `session` or `visitor`
- **THEN** Trace refuses and lists the breakdowns it offers

### Requirement: Where queries are available
Queries SHALL be available as the MCP tool `query_behaviour`, to every token including read-only ones, and at `/api/sites/{site}/query` for signed-in people and tokens.

#### Scenario: A read-only agent
- **WHEN** an agent connected with read-only access asks a question
- **THEN** it gets the answer
