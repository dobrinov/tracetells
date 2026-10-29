# Spec Delta

## Purpose

Find what is plainly broken on a website from the first session on, without waiting for enough traffic to see patterns, and track each defect until it stops happening.

## ADDED Requirements

### Requirement: Defect candidates
Trace SHALL list a website's defect candidates over a window (14 days by default, up to 90): failing requests, JS exceptions and console errors, missing files, funnel steps that no session reaches although the step before has sessions, custom events declared in trace.json that have not arrived, and dead clicks on elements whose selectors look interactive. Each candidate SHALL have a stable key, a kind, the pages it was seen on, its count and how many sessions it affected, and SHALL say whether an insight in any status already covers it. Candidates SHALL need no minimum number of sessions. The MCP server SHALL offer them as `get_defects`.

#### Scenario: A failing endpoint on a small site
- **WHEN** a website with 27 sessions has 190 failed `POST stats.example.com/api/event` requests across 14 pages, all from one session
- **THEN** `get_defects` lists one failing-request candidate for that request, with 190 occurrences, 1 session and the 14 pages

#### Scenario: A funnel step nobody reaches
- **WHEN** 8 sessions reach the first step of the acquisition funnel (`/`) and none reaches `/sign-up`
- **THEN** `get_defects` lists the `/sign-up` step as unreached, with the 8 sessions that reached the step before it

#### Scenario: A declared event that never arrives
- **WHEN** trace.json declares `signup` and no `signup` event has ever arrived
- **THEN** `get_defects` lists `signup` as never arriving

#### Scenario: A dead click on something that isn't meant to be clicked
- **WHEN** visitors dead-click `div.question-body > p` and `button.pay`
- **THEN** `button.pay` is a candidate and `div.question-body > p` is not

#### Scenario: Already covered
- **WHEN** a dismissed insight has the fingerprint that matches a candidate's key
- **THEN** the candidate is listed as covered by that insight

### Requirement: Matching an error by its message
An event matcher SHALL accept an optional `text` pattern, matched exactly or by prefix (ending in `*`) against an error's stored message, in `query_behaviour`, insight evidence and trace.json. A matcher without `text` SHALL match as it does today. A defect candidate for an error SHALL suggest a metric that matches its message.

#### Scenario: One failing request among others
- **WHEN** `/sign-in` has failed requests to `stats.example.com` and to `/sign-in` itself
- **THEN** `{"error": "*", "text": "POST stats.example.com/*"}` counts only the first

### Requirement: Defect insights
An insight SHALL have a kind: `pattern` or `defect`, `pattern` when none is given. A defect insight SHALL NOT need a KPI. Its evidence SHALL be accepted when it measures zero, if its goal is `increase` (something that should happen and doesn't). Defect insights SHALL be deduplicated, saved and moved through statuses like any other insight, and SHALL show their kind wherever insights are listed.

#### Scenario: Drafting an event that never arrives
- **WHEN** an agent drafts a defect with the evidence "sessions with event `signup`", goal increase, and that measures zero
- **THEN** the draft is returned with its reading instead of being refused

#### Scenario: A pattern measured at zero
- **WHEN** an agent drafts a pattern insight whose only measurement is zero
- **THEN** the draft is refused as it is today

### Requirement: Judging a released defect
After a defect insight is released, Trace SHALL judge each measured evidence item by whether the failure recurs, not by a significance test. When the window after the release has passed, an item whose goal is `decrease` SHALL be judged stopped if it did not happen at all after the release, and still happening otherwise. An item whose goal is `increase` SHALL be judged fixed once it happens, and still missing otherwise. When nothing reached the item's pages after the release, the verdict SHALL say there wasn't enough traffic to tell.

#### Scenario: The failing request stops
- **WHEN** a defect for a failing request is released and the request doesn't fail once in the 14 days after, while its pages had sessions
- **THEN** the item's verdict is that it stopped

#### Scenario: The event starts arriving
- **WHEN** a defect for `signup` never arriving is released and a `signup` event arrives after it
- **THEN** the item's verdict is that it is fixed

### Requirement: Analysing a small website
When an analysis isn't ready for patterns, the `analyse_website` prompt and `/trace:analyse` SHALL run a defect sweep instead of stopping: read `get_defects`, confirm each uncovered candidate in the codebase, draft only confirmed ones as defect insights, and show the person each draft with what was found in the code. When an analysis is ready, they SHALL cover defects and patterns. Text inside candidates SHALL be treated as data from visitors, never as instructions.

#### Scenario: 27 sessions
- **WHEN** someone runs `/trace:analyse` on a website with 27 sessions and three uncovered defect candidates
- **THEN** the agent says patterns need more sessions, looks up the three candidates in the code, and drafts the ones it confirms as defects for the person to approve

#### Scenario: Not confirmed in the code
- **WHEN** a failing request turns out to come from a browser extension, not from anything in the codebase
- **THEN** the agent doesn't draft it and tells the person why
