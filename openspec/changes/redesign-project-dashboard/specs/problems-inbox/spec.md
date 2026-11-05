# Spec Delta

## Purpose

Puts everything that goes wrong for a website's visitors in one place, by page, so a team sees where it hurts most and can hand each problem to its coding agent.

## ADDED Requirements

### Requirement: Problems by page
Problems SHALL list, for the last 7, 14 or 30 days, every page with a dead click, rage click or browser error, with how many of each happened and how many sessions hit any of them. Pages SHALL be ordered by sessions hit, most first, at most 50.

#### Scenario: The questions page
- **WHEN** `/questions/:id` had 247 dead clicks, 81 rage clicks and 4 browser errors across 19 sessions
- **THEN** its row shows 247, 81, 4 and 19, above pages hit in fewer sessions

### Requirement: Shading by how much
Each count SHALL be shaded by its size relative to the largest in its column, so the worst cells stand out, and the shades SHALL also differ in lightness, not hue alone.

#### Scenario: Most dead clicks
- **WHEN** one page has 247 dead clicks and another 11
- **THEN** the first cell is the darkest in its column and the second light

### Requirement: A problem in detail
Choosing a page SHALL show its problems in detail:
- each dead or rage clicked element with its label, each browser error with its message and kind, with events and sessions
- the insights that already name the page
- a link to the page's story

Each problem SHALL offer a task for the coding agent to copy, as area pages do today, and SHALL NOT be dismissed or changed from the dashboard.

#### Scenario: Handing it over
- **WHEN** someone chooses `/questions/:id` and copies the task for its writing-field dead clicks
- **THEN** the copied text names the page, the element, the counts and the window, and asks the agent to find the cause and save it as an insight

### Requirement: Server errors next to them
Problems SHALL also list the website's open server errors, most harmful first, linking to each, and say how to start reporting them when there are none.

#### Scenario: No server errors yet
- **WHEN** a website has never reported a server error
- **THEN** Problems says server errors aren't reported yet and links to setting them up
