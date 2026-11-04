# funnels Specification

## Purpose
Funnels show the steps people take in order within one visit and where most of them are lost, declared in trace.json or recognised from KPIs that chain.

## Requirements

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

#### Scenario: Analysing a funnel
- **WHEN** most sessions in a funnel are lost between its first and second steps
- **THEN** `get_funnels` names the second step as the biggest drop, and the analysis brief tells the agent to start there, with that step's conversion as evidence to increase
