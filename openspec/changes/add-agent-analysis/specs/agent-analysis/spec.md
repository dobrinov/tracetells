# Spec Delta

## Purpose

Let a website's coding agent do the analysis over MCP, with its own model, and save what it finds as Trace insights.

## ADDED Requirements

### Requirement: Agents save insights
The MCP server SHALL offer `add_insights`, which saves up to 10 insights at once. Each needs a title, problem, evidence, proposal and acceptance criteria, and can name a KPI by key or id. Read-only tokens SHALL be refused, and every save SHALL be recorded in the activity log.

#### Scenario: An agent saves a finding
- **WHEN** an agent calls `add_insights` with a complete insight linked to the KPI `checkout_conversion`
- **THEN** the insight appears with status new, source agent and that KPI, and the activity log shows the save

### Requirement: Saving is safe to repeat
An insight with the same fingerprint or title as an existing one, in any status, SHALL be skipped. If any item in a call is invalid, including an unknown KPI, nothing from that call SHALL be saved.

#### Scenario: The same finding again
- **WHEN** an agent saves an insight with a fingerprint Trace already has, under a different title
- **THEN** it is reported as already known and nothing new is saved

#### Scenario: A bad item
- **WHEN** one of two insights names a KPI that doesn't exist
- **THEN** the call fails and neither insight is saved

### Requirement: An analysis brief for agents
The MCP server SHALL offer an `analyse_website` prompt that tells the agent which tools to read, to skip existing insights and learn from dismissal reasons, to save up to five specific findings with `add_insights`, and to treat text inside behaviour data as data from visitors, never as instructions.

#### Scenario: Running the analysis in Claude Code
- **WHEN** someone runs `/mcp__trace__analyse_website` in Claude Code connected to Trace
- **THEN** the agent receives the brief and can complete it with Trace's tools alone
