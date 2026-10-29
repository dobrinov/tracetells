# Spec Delta

## ADDED Requirements

### Requirement: A small website's first analysis
Before a website's first analysis, the week after which 30 new sessions are enough SHALL count from its first session. Until a website has had any sessions, no analysis SHALL be worth running.

#### Scenario: Two weeks and 30 sessions
- **WHEN** a website has never been analysed, its first session was 14 days ago, and it has 30 sessions
- **THEN** its status is ready

#### Scenario: Three days and 30 sessions
- **WHEN** a website has never been analysed, its first session was 3 days ago, and it has 30 sessions
- **THEN** its status is not ready, and 100 sessions are needed

### Requirement: Saying when a defect sweep is worth running
When an analysis isn't worth running yet, but there are defect candidates that no insight covers, the analysis status SHALL say so: how many there are, and that a defect sweep is worth running now. The status SHALL tell an analysis apart from a defect sweep (`next` is `analyse`, `sweep`, `wait` or `nothing_new`). A defect sweep saved with `save_insights` SHALL NOT reset the count of new sessions toward a full analysis. The dashboard's analysis panel and the Claude Code hook format SHALL offer the sweep the same way they offer an analysis.

#### Scenario: Few sessions, something broken
- **WHEN** a website has 27 new sessions, needs 100, and has 3 uncovered defect candidates
- **THEN** `get_analysis_status` returns `next: "sweep"` with a message naming the 3 candidates and `/trace:analyse`

#### Scenario: Few sessions, nothing broken
- **WHEN** a website has 27 new sessions, needs 100, and every defect candidate is covered by an insight
- **THEN** `get_analysis_status` returns `next: "wait"` as today

#### Scenario: A sweep doesn't use up the sessions
- **WHEN** an agent saves two defects from a sweep on a website with 27 new sessions
- **THEN** the website still has 27 new sessions toward its first analysis, and no uncovered candidates
