# Spec Delta

## ADDED Requirements

### Requirement: An area is used when it or its features match
A session SHALL count for an area when any event in it matches the area's own matchers or any of its features' matchers, and for a feature when it matches the feature's.

#### Scenario: Joining a league
- **WHEN** a session joins a league, declared as the feature `leagues` of the area `battles`
- **THEN** it counts for both `leagues` and `battles`

### Requirement: Problems belong to an area's own pages
A problem (rage click, dead click or error) SHALL count for an area only when it happened in a session using the area, on a page matched by the area's pageview patterns or where its other events happened.

#### Scenario: An error elsewhere
- **WHEN** a session that dueled hits an error on a practice page
- **THEN** the error counts for Practice, not for Battles

### Requirement: Status points attention
An area or feature with fewer than 10 sessions SHALL be "too little data". Otherwise it is painful from 15% of its sessions with a problem, and rare below 5% reach (a feature: below 10% of its area's sessions).

#### Scenario: Busy and broken
- **WHEN** 4 of 13 sessions using Battles rage-click the ready button
- **THEN** Battles is "used, but it hurts", with that rage click as its most common problem, and Home raises it
