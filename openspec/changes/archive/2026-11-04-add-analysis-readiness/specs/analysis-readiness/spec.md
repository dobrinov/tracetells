# Spec Delta

## Purpose

Tell a website's owner, and their coding agent, when enough new behaviour has arrived to be worth analysing.

## ADDED Requirements

### Requirement: When an analysis is worth running
Trace SHALL count the sessions that started since a website was last analysed. It SHALL call an analysis worth running at 100 new sessions, or at 30 once a week has passed since the last analysis. An agent saving with `add_insights`, even an empty list, or Trace's own analysis SHALL count as an analysis.

#### Scenario: Enough new behaviour
- **WHEN** a website has 140 new sessions since yesterday's analysis
- **THEN** its status is ready, with a message naming the count and how to run the analysis

#### Scenario: An agent finds nothing
- **WHEN** an agent calls `add_insights` with an empty list
- **THEN** the website counts as analysed and the count of new sessions starts again

### Requirement: Saying so where the owner works
The status SHALL appear on the Insights page, in MCP `get_overview` and `get_analysis_status`, and at `/api/sites/{site}/analysis`. With `?format=text` or `?format=claude-hook`, that endpoint SHALL return nothing unless an analysis is worth running.

#### Scenario: A Claude Code hook
- **WHEN** the Setup page's SessionStart hook runs while the website is ready for analysis
- **THEN** Claude Code shows the person a line saying so; when it isn't ready, the hook prints nothing
