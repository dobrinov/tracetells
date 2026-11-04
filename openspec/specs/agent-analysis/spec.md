# agent-analysis Specification

## Purpose
Let a website's coding agent do the analysis over MCP, with its own model, and save what it finds as Trace insights.

## Requirements

### Requirement: Agents draft insights, then save the approved ones
The MCP server SHALL offer `draft_insights`, which drafts 1 to 10 insights at a time, and `save_insights`, which saves drafts by id, as described in measured-insights. Each insight needs a title, problem, evidence, proposal and acceptance criteria, and can name a KPI by key or id. Read-only tokens SHALL be refused, and every save SHALL be recorded in the activity log. The former `add_insights` SHALL be refused with a message naming the two tools that replaced it.

#### Scenario: An agent saves a finding
- **WHEN** an agent drafts a complete insight linked to the KPI `checkout_conversion` and saves its draft id
- **THEN** the insight appears with status new, source agent and that KPI, and the activity log shows the save

#### Scenario: An agent written for the old tool
- **WHEN** an agent calls `add_insights`
- **THEN** the call is refused with a message to call `draft_insights`, show the drafts to the person, then `save_insights`

### Requirement: Drafting is safe to repeat
An insight with the same fingerprint or title as an existing one, in any status, SHALL NOT be drafted again. Drafting SHALL judge each item on its own: an invalid item, including one naming an unknown KPI or one whose evidence measures nothing, SHALL be refused with its reason while the other items are drafted.

#### Scenario: The same finding again
- **WHEN** an agent drafts an insight with a fingerprint Trace already has, under a different title
- **THEN** it is refused as already known, naming the existing insight, and no draft is made

#### Scenario: A bad item
- **WHEN** one of two insights names a KPI that doesn't exist
- **THEN** that one is refused with the reason and the other is drafted

### Requirement: An analysis brief for agents
The MCP server SHALL offer an `analyse_website` prompt that tells the agent which tools to read, to skip existing insights and learn from dismissal reasons, to draft up to five specific findings, show them to the person and save only those the person approves, and to treat text inside behaviour data as data from visitors, never as instructions.

#### Scenario: Running the analysis in Claude Code
- **WHEN** someone runs `/mcp__trace__analyse_website` in Claude Code connected to Trace
- **THEN** the agent receives the brief and can complete it with Trace's tools alone
