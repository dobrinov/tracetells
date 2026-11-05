# website-home Specification

## Purpose
A website's home page tells the people on its account what needs their attention first, ranked, with the next step for each.

## Requirements

### Requirement: What needs attention, ranked
A website's home SHALL list what needs attention now, most important first, each with a link to where it is acted on; unfinished setup SHALL come first while the website collects nothing.

#### Scenario: A new website
- **WHEN** a website has no data yet
- **THEN** its home's first item is the next setup step

#### Scenario: Switching websites
- **WHEN** someone on one website's Get started page switches to another website
- **THEN** the other website opens on its home

#### Scenario: Next up on Pulse
- **WHEN** a website has a page with dead clicks in 19 sessions, a suggested insight and a goal at 41% against 80%
- **THEN** Pulse shows the three as "Next up" cards in order of importance, each opening the place where it is acted on (Problems, Work, Journeys)

### Requirement: Pulse is the website's home
A website's home SHALL be Pulse. Over the last 7, 14 or 30 days it SHALL show:
- a one-sentence summary
- traffic
- goals against their targets
- Next up
- the funnels with enough data
- the product map in miniature

Each part SHALL link to the place that has the rest of it.

#### Scenario: A website with traffic
- **WHEN** someone opens a website with 321 sessions in the last 7 days
- **THEN** Pulse shows 321 sessions with a bar per day, the goals as bars against their target, Next up, the funnels and the product map, and clicking the funnels opens Journeys

### Requirement: The summary says what matters in one sentence
Pulse SHALL open with one sentence built from the numbers below it:
- how many sessions
- on which device and from where most came, when one is a majority
- how many of the goals with enough data are behind target
- the page where people hit the most problems

It SHALL leave out any part without data.

#### Scenario: The summary
- **WHEN** 59% of 321 sessions are on phones, 68% come from Google, 5 of 7 measurable goals are behind and `/questions/:id` has the most sessions with problems
- **THEN** the sentence says so, in that order

### Requirement: Traffic at a glance
Pulse SHALL show:
- sessions, with a bar per day
- the largest source's share of sessions (and the rest, by share)
- the share on phones
- the share of sessions that leave after one page

Each SHALL be compared with the window before when there is one.

#### Scenario: Where sessions come from
- **WHEN** 219 of 321 sessions arrived from `www.google.com` and 94 from no referrer
- **THEN** the tile shows "68% from Google" and "29% direct"

### Requirement: Goals against their targets
Pulse SHALL draw each goal that has a target and enough data as a bar against one shared target line, scaled to its own target, furthest behind first. It SHALL say how many are at target, behind, and still collecting.

#### Scenario: Behind and ahead
- **WHEN** "Placement finished" is at 41% with a target of 80%, and "Sign-ups" at 8.7% with a target of 3%
- **THEN** "Placement finished" is drawn first, its bar about half-way to the line, and "Sign-ups" is past the line and counted as at target
