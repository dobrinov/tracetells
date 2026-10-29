# Spec Delta

## ADDED Requirements

### Requirement: Self-measurement is opt-in
The dashboard SHALL load a collector only when the server has both a dashboard write key and secret, and SHALL never send page names containing ids or website names.

#### Scenario: A self-hosted server
- **WHEN** Trace runs without `TRACE_DASHBOARD_KEY`
- **THEN** the dashboard page loads no collector and the auth state has no analytics id

### Requirement: Recent reports are served from memory
A report requested again within a minute SHALL be served without recomputing, and a change made through the API SHALL drop that website's reports.

#### Scenario: Moving an insight
- **WHEN** someone releases an insight and opens Home
- **THEN** Home is recomputed and shows the release
