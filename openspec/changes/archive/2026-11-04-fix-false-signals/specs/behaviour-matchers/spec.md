# Spec Delta

## Purpose
Event matchers say what a KPI, a funnel step, an insight's evidence or a `query_behaviour` question counts. A matcher that can never match must be refused with a reason, not answered with a silent zero.

## ADDED Requirements

### Requirement: Page patterns start with a slash
Wherever a matcher names a page, through the shorthand value of a page-based type (for example `{"pageview": ...}` or `{"error": ...}`) or through `"path"`, the pattern SHALL be `*` or start with `/`. Any other pattern SHALL be refused, in trace.json checks and in `query_behaviour`, with a message that page patterns start with `/` and, for errors and clicks, that the message or label to match goes in `"text"`.

#### Scenario: An error message given as the page
- **WHEN** an agent asks `query_behaviour` for sessions matching `{"error": "Missing target*"}`
- **THEN** the call is refused with a message to use `{"error": "*", "text": "Missing target*"}`

#### Scenario: The same mistake in trace.json
- **WHEN** a trace.json KPI uses `{"session_rate": {"error": "TypeError*"}}`
- **THEN** checking the file reports that KPI's matcher, with the same hint

#### Scenario: A valid page pattern
- **WHEN** a matcher is `{"error": "/checkout"}` or `{"pageview": "/products/*"}`
- **THEN** it is accepted and matches as before

### Requirement: A matcher has no keys it ignores
A matcher SHALL be refused, in trace.json checks and in `query_behaviour`, when it has a key that would not be used: in the `type` form, anything but `type`, `path`, `selector`, `name` and `text`; in the shorthand form, more than one event type. The message SHALL name the offending key and the keys allowed, and SHALL say to use `"text"` when the key is `message`.

#### Scenario: An error message under the wrong key
- **WHEN** an agent asks `query_behaviour` for events matching `{"type": "error", "message": "TypeError*"}`
- **THEN** the call is refused, naming `message` and suggesting `"text"`, instead of counting every error

#### Scenario: Two keys in the shorthand
- **WHEN** a trace.json funnel step is `{"error": "*", "name": "X"}`
- **THEN** checking the file reports that step, naming `name` as the key it doesn't recognise

### Requirement: Error messages are matched with text
A matcher's `"text"` SHALL match an error's whole message as stored, with a trailing `*` matching by prefix, and the `query_behaviour` description SHALL show an example of it. Messages of unhandled rejections begin with `Unhandled rejection: `, and the description SHALL say so.

#### Scenario: Matching an unhandled rejection
- **WHEN** an agent queries `{"error": "*", "text": "Unhandled rejection: Missing target*"}`
- **THEN** the sessions with that rejection are counted

#### Scenario: Text that matches nothing
- **WHEN** an agent queries `{"type": "error", "text": "no-such-message*"}` on a website with errors
- **THEN** the answer is 0, not the number of all errors
