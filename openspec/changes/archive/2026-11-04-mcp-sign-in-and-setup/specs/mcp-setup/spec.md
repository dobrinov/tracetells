# Spec Delta

## Purpose

Let a coding agent set a website up from the codebase through MCP.

## ADDED Requirements

### Requirement: Setup tools
MCP SHALL offer `add_site`, `get_install_snippet`, `get_setup_status`, `push_config`, `add_privacy_rule`, `remove_privacy_rule` and `confirm_privacy_review`. Tools that change setup SHALL need the admin role and a token that isn't read-only, and SHALL be recorded in the activity log.

#### Scenario: A new website from the codebase
- **WHEN** an admin's agent calls `add_site` with "shop.example.com"
- **THEN** the website exists and the result includes its script tag for this server

#### Scenario: Privacy rules reach browsers
- **WHEN** an agent adds a privacy rule through the server's `/mcp`
- **THEN** the collector the server serves includes it at once
