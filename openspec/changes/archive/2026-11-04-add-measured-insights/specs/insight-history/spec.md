# Spec Delta

## Purpose

Keep an insight's evidence up to date, keep every measurement, and show whether the problem it describes went away after a change shipped.

## ADDED Requirements

### Requirement: Refreshing evidence
Trace SHALL re-measure the measured evidence of every insight that isn't dismissed:
- once a day;
- on demand, through the MCP tool `refresh_insight`, the API, and a Refresh button on the insight.

Each refresh SHALL measure over the same number of days as the insight's first measurement, ending now. A refresh SHALL NOT change the insight's text.

#### Scenario: On demand
- **WHEN** someone refreshes an insight first measured over 14 days
- **THEN** each of its measurements is taken again over the last 14 days, and the new values are shown with today's date

### Requirement: History
Every measurement SHALL be kept with its value, session count, window and time taken. The dashboard, the API and MCP SHALL show each evidence item's history in time order.

#### Scenario: A history
- **WHEN** an insight has been refreshed daily for a week
- **THEN** its evidence shows eight measurements per item: the first and seven refreshes

### Requirement: Evidence around a release
For a released insight, Trace SHALL measure each evidence item over the 14 days before the release and the 14 days after. It SHALL say per item whether it moved in the direction the insight wants (the problem shrank) or the other way, with the same significance test and minimum sample as KPI verdicts.

Until 14 days have passed, it SHALL say it is measuring. The result SHALL appear next to the KPI verdict and SHALL settle once, like the KPI verdict.

#### Scenario: The problem went away
- **WHEN** an insight about rage clicks on Pay now is released, and the rage-click rate falls from 18% to 2% across the two windows
- **THEN** that evidence item shows it improved, with both numbers and the session counts, next to the checkout conversion verdict

#### Scenario: Better locally, worse overall
- **WHEN** the evidence improved but the KPI regressed
- **THEN** both verdicts are shown as they are; neither hides the other

### Requirement: Built-in insights are measured too
Insights from Trace's built-in checks and its Claude analyst SHALL carry measured evidence, so they are refreshed and judged the same way.

#### Scenario: A built-in rage-click insight
- **WHEN** the built-in checks find rage clicks on an element
- **THEN** the insight's evidence includes the measured rage-click rate for that element and page, and it is refreshed daily
