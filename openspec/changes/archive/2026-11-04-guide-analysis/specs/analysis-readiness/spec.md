# Spec Delta

## ADDED Requirements

### Requirement: The dashboard says when and how to analyse
Home and Insights SHALL show when the last analysis ran and who ran it, the sessions and events recorded since, and the threshold. When it's worth analysing, they SHALL show the command to run in the coding agent, or how to connect one.

#### Scenario: Behaviour has piled up
- **WHEN** a website has 398 new sessions since Claude Code analysed it three days ago
- **THEN** Home shows "Run an analysis now", "3 days ago by Claude Code", "398 sessions" with their events, and `/trace:analyse` with a Copy button, and the header button reads "Run analysis · 398 new"

#### Scenario: The agent saves an analysis while the page is open
- **WHEN** the panel asks for an analysis and the agent saves one
- **THEN** within 20 seconds the page says it was analysed and the panel resets
