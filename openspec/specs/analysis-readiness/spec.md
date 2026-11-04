# analysis-readiness Specification

## Purpose
Tell a website's owner, and their coding agent, when enough new behaviour has arrived to be worth analysing.

## Requirements

### Requirement: When an analysis is worth running
Trace SHALL count the sessions that started since a website was last analysed. It SHALL call an analysis worth running at 100 new sessions, or at 30 once a week has passed since the last analysis. Saving with `save_insights`, even no drafts, SHALL count as an analysis when one was worth running or a pattern insight is saved; otherwise it SHALL count as a defect sweep, which leaves the count of new sessions as it was.

#### Scenario: Enough new behaviour
- **WHEN** a website has 140 new sessions since yesterday's analysis
- **THEN** its status is ready, with a message naming the count and how to run the analysis

#### Scenario: An agent finds nothing
- **WHEN** an analysis is worth running and an agent calls `save_insights` with no drafts
- **THEN** the website counts as analysed and the count of new sessions starts again

### Requirement: Saying so where the owner works
The status SHALL appear on the Insights page, in MCP `get_overview` and `get_analysis_status`, and at `/api/sites/{site}/analysis`. With `?format=text` or `?format=claude-hook`, that endpoint SHALL return nothing unless an analysis is worth running.

#### Scenario: A Claude Code hook
- **WHEN** the Setup page's SessionStart hook runs while the website is ready for analysis
- **THEN** Claude Code shows the person a line saying so; when it isn't ready, the hook prints nothing

### Requirement: The dashboard says when and how to analyse
Home and Insights SHALL show when the last analysis ran and who ran it, the sessions and events recorded since, and the threshold. When it's worth analysing, they SHALL show the command to run in the coding agent, or how to connect one.

#### Scenario: Behaviour has piled up
- **WHEN** a website has 398 new sessions since Claude Code analysed it three days ago
- **THEN** Home shows "Run an analysis now", "3 days ago by Claude Code", "398 sessions" with their events, and `/trace:analyse` with a Copy button, and the header button reads "Analyse · 398 new"

#### Scenario: The agent saves an analysis while the page is open
- **WHEN** the panel asks for an analysis and the agent saves one
- **THEN** within 20 seconds the page says it was analysed and the panel resets

### Requirement: A small website's first analysis
Before a website's first analysis, the week after which 30 new sessions are enough SHALL count from its first session. Until a website has had any sessions, no analysis SHALL be worth running.

#### Scenario: Two weeks and 30 sessions
- **WHEN** a website has never been analysed, its first session was 14 days ago, and it has 30 sessions
- **THEN** its status is ready

#### Scenario: Three days and 30 sessions
- **WHEN** a website has never been analysed, its first session was 3 days ago, and it has 30 sessions
- **THEN** its status is not ready, and 100 sessions are needed

### Requirement: Saying when a defect sweep is worth running
When an analysis isn't worth running yet but some defect candidates aren't covered by any insight, the analysis status SHALL say how many there are and that a defect sweep is worth running now, with `next` one of `analyse`, `sweep`, `wait` or `nothing_new`. A sweep saved with `save_insights` SHALL NOT reset the count of new sessions toward a full analysis. The dashboard's analysis panel and the Claude Code hook SHALL offer a sweep as they offer an analysis.

#### Scenario: Few sessions, something broken
- **WHEN** a website has 27 new sessions, needs 100, and has 3 uncovered defect candidates
- **THEN** `get_analysis_status` returns `next: "sweep"` with a message naming the 3 candidates and `/trace:analyse`

#### Scenario: Few sessions, nothing broken
- **WHEN** a website has 27 new sessions, needs 100, and every defect candidate is covered by an insight
- **THEN** `get_analysis_status` returns `next: "wait"` as today

#### Scenario: A sweep doesn't use up the sessions
- **WHEN** an agent saves two defects from a sweep on a website with 27 new sessions
- **THEN** the website still has 27 new sessions toward its first analysis, and no uncovered candidates
