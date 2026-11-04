# Spec Delta

## MODIFIED Requirements

### Requirement: Defect candidates
Trace SHALL list a website's defect candidates over a window (14 days by default, up to 90), with no minimum number of sessions: failing requests, JS exceptions and console errors, missing files, funnel steps no session reaches after the step before, declared custom events that never arrived, and dead clicks on elements that look interactive. Each SHALL have a stable key, kind, pages, count and sessions affected, and say whether an insight in any status covers it. The MCP tool is `get_defects`.

#### Scenario: A failing endpoint on a small site
- **WHEN** a website with 27 sessions has 190 failed `POST /api/event` requests to its own API across 14 pages, all from one session
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
