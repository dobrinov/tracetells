# Spec Delta

## ADDED Requirements

### Requirement: Old links keep working
Links to the pages before the redesign SHALL open the page that replaced them.

#### Scenario: A bookmarked KPIs page
- **WHEN** someone opens `#/kpis`
- **THEN** Goals opens, at `#/improve/goals`

### Requirement: Onboarding counts outcomes
Setup SHALL be complete when the first visit arrived, goals are applied and a first analysis ran; connecting an agent, the privacy review and inviting the team SHALL NOT block it.

#### Scenario: No coding agent
- **WHEN** a website has data, goals and an analysis run from the dashboard, and nobody connected an agent
- **THEN** its setup is complete

### Requirement: Analysis can focus on one area
The analyse prompt SHALL accept an area or feature key and limit the analysis to its pages.

#### Scenario: Analyse this
- **WHEN** someone clicks "Analyse this" on the Product page feature
- **THEN** `/trace:analyse product` is copied, and the agent analyses only that feature
