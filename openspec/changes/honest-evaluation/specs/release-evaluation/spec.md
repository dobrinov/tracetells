# Spec Delta

## Purpose

Judge whether a released insight moved its KPI without letting repeated checks, later KPI edits or a wrong release time decide the answer.

## ADDED Requirements

### Requirement: A fixed window, then a settled verdict
Trace SHALL compare a released insight's KPI over the 14 days before its release time with the 14 days after. Before the 14 days after have passed it SHALL report `measuring` with no values. After that it SHALL store one verdict and SHALL NOT recompute it unless the release time or the linked KPI changes.

#### Scenario: Three days after a release
- **WHEN** an insight was released 2 days and some hours ago
- **THEN** its evaluation is "Measuring: day 3 of 14" with the date the verdict comes

#### Scenario: Data after the verdict
- **WHEN** an evaluation has settled and more sessions arrive
- **THEN** the verdict and its numbers stay the same

### Requirement: Judged by the KPI as shipped
Releasing an insight SHALL store its KPI's definition and direction at that moment, and the evaluation SHALL use them. When the current definition differs, the summary SHALL say the release was measured with the earlier one.

#### Scenario: The goal moves after the release
- **WHEN** `trace.json` changes a KPI's goal page after an insight linked to it shipped
- **THEN** that insight is still measured with the old goal, and its summary says so

### Requirement: The time the change went live
Marking an insight released SHALL accept the time it went live. Trace SHALL refuse a time in the future or before the insight was created. `POST /api/sites/{site}/releases` SHALL mark a list of insights released at one time, and report each it skipped with the reason.

#### Scenario: A deploy pipeline
- **WHEN** CI posts `{"insights": [12, 7], "at": "2026-10-01T09:30:00Z"}` and #7 is dismissed
- **THEN** #12 is released at that time and #7 is listed as skipped, "dismissed"

### Requirement: Overlapping releases are named
A settled verdict SHALL list other releases on the same KPI whose release falls within 14 days of this one.

#### Scenario: Two fixes to checkout
- **WHEN** two insights linked to checkout conversion ship two days apart
- **THEN** each verdict names the other
