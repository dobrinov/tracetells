# Spec Delta

## ADDED Requirements

### Requirement: Ordered steps within a session
A funnel step SHALL count a session only when it reached that step at or after reaching the step before, in the same session.

#### Scenario: Out of order
- **WHEN** a session signs up and then views the pricing page
- **THEN** it counts for the pricing step of a funnel `/pricing → signup` but not for its signup step

### Requirement: Recognised from KPIs
Conversion KPIs whose goal is another's start SHALL form a funnel of three or more steps, unless a declared funnel contains the same steps in order.

#### Scenario: Two chained KPIs
- **WHEN** KPIs measure `/` → `/pricing` and `/pricing` → `signup`
- **THEN** the KPIs page shows the funnel `/` → `/pricing` → `signup`, recognised from those KPIs

### Requirement: Analysis starts from the biggest drop
Analysis SHALL be given each funnel's biggest drop, and insights about a funnel step SHALL carry that step's conversion as measured evidence.

#### Scenario: Built-in checks
- **WHEN** a funnel step loses at least 10 sessions and fewer than 70% continue
- **THEN** Trace's built-in analysis proposes an insight about that step, with the step's conversion as evidence to increase
