# Spec Delta

## ADDED Requirements

### Requirement: The server never analyses on its own
Trace SHALL NOT run analysis on the server; insights SHALL be created only through MCP by the person's agent.

#### Scenario: Analysis is due
- **WHEN** a website has 100 new sessions since the last analysis
- **THEN** Home and Insights show `/trace:analyse` to copy, and nothing on the dashboard starts an analysis

### Requirement: Changes go through the agent
The dashboard SHALL NOT change insights, privacy rules, the privacy review, the public link or websites' existence (except deleting one); where such a change is relevant it SHALL show the command to ask the agent.

#### Scenario: An insight in progress
- **WHEN** someone views an insight that's in progress
- **THEN** its card shows `/trace:manage release insight <id>` with Copy, and no button that changes it
