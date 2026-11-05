# dashboard Specification

## Purpose
The dashboard shows a website's aggregates to the people on its account, quickly and at stable addresses, and leaves analysis and changes to the team's coding agent.

## Requirements

### Requirement: Old links keep working
Links to the pages before the redesign SHALL open the page that replaced them. This holds for both earlier layouts: the flat pages (`#/kpis`, `#/behaviour`) and the four areas (`#/home`, `#/understand/…`, `#/improve/…`, `#/setup/…`).

#### Scenario: A bookmarked KPIs page
- **WHEN** someone opens `#/kpis`
- **THEN** Goals opens, at `#/goals`

#### Scenario: A link to a page's story
- **WHEN** someone opens `#/understand/pages/%2Fcheckout`
- **THEN** the story of `/checkout` opens, at `#/map/pages/%2Fcheckout`

#### Scenario: Old areas
- **WHEN** someone opens `#/home`, `#/understand/audience`, `#/understand/errors`, `#/improve/releases` or `#/setup/privacy`
- **THEN** Pulse, Pulse, Problems, Work and Settings › Privacy open, each at its new address

### Requirement: Onboarding counts outcomes
Setup SHALL be complete when a coding agent is connected, the first visit arrived, goals are applied and a first analysis ran or insight was saved; the privacy review, inviting the team and server errors SHALL NOT block it.

#### Scenario: Everything but the optional steps
- **WHEN** an agent is connected and a website has data, goals and an insight, with no privacy review and nobody invited
- **THEN** its setup is complete

### Requirement: Analysis can focus on one area
The analyse prompt SHALL accept an area or feature key and limit the analysis to its pages.

#### Scenario: Analyse this
- **WHEN** someone clicks "Analyse this" on the Product page feature
- **THEN** `/trace:analyse product` is copied, and the agent analyses only that feature

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

### Requirement: The server never analyses on its own
Trace SHALL NOT run analysis on the server; insights SHALL be created only through MCP by the person's agent.

#### Scenario: Analysis is due
- **WHEN** a website has 100 new sessions since the last analysis
- **THEN** Home and Insights show `/trace:analyse` to copy, and nothing on the dashboard starts an analysis

### Requirement: Changes go through the agent
The dashboard SHALL NOT change insights, privacy rules, the privacy review, the public link or websites' existence (except deleting one, or adding one to an account that has none); where such a change is relevant it SHALL show the command to ask the agent.

#### Scenario: An insight in progress
- **WHEN** someone views an insight that's in progress
- **THEN** its card shows `/trace:manage release insight <id>` with Copy, and no button that changes it
