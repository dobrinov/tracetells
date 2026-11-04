# Spec Delta

## ADDED Requirements

### Requirement: Every client gets the guided workflows
The MCP server SHALL offer setup, analyse, fix and manage prompts whose instructions are the Claude Code plugin's.

#### Scenario: Cursor
- **WHEN** a Cursor user picks the manage_trace prompt with "release insight 12"
- **THEN** it receives the same instructions as /trace:manage, starting with that request
