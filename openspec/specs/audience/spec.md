# audience Specification

## Purpose
Trace counts who comes to a website (visitors, bounces on marketing pages, engaged visitors, active people and retention) from aggregates alone, without following any one person.

## Requirements

### Requirement: Visitors are counted per day
Anonymous visitors SHALL be counted once per day, and a window's visitors SHALL be the sum of its days; a signed-in person SHALL count once per day under their id.

#### Scenario: A returning anonymous visitor
- **WHEN** the same anonymous visitor comes on Monday and Tuesday
- **THEN** the week's visitors count them twice, and the page says why

### Requirement: Bounce on the marketing pages
A session SHALL count as a bounce when it landed on a page matching `audience.marketing` (any page when none is declared) and had no other pageview and no click, form start, submit or custom event.

#### Scenario: Landed and clicked
- **WHEN** a session lands on `/` and clicks a link that opens another site
- **THEN** it counts as landed on a marketing page but not as a bounce

### Requirement: Engaged visitors
The engaged share SHALL be the visitors (per day) with at least one event matching `audience.engaged`, over all visitors; without declared actions it SHALL not be measured.

#### Scenario: Nothing declared
- **WHEN** trace.json has no `audience.engaged`
- **THEN** the Audience page shows how to declare it instead of a rate

### Requirement: Active people and retention need sign-in
DAU, WAU, MAU, stickiness and retention SHALL count only ids passed to `trace.identify()`; without any, the page SHALL explain that sign-in is needed.

#### Scenario: Retention cohort
- **WHEN** a person is first signed in during a week and seen again the week after
- **THEN** their cohort's week 1 counts them, and a week shows only once it is over

### Requirement: Where sessions come from
The audience report SHALL give the share of sessions by where they came from: the referrer's origin, or "direct" when there was none. Search engines, and an origin's `www.` and `m.` forms, SHALL be counted as one source (google.com and google.bg are Google). Sources from fewer than 5 sessions SHALL be counted together as "other". Each share SHALL come with the window before.

#### Scenario: Search and direct
- **WHEN** 219 sessions came from `https://www.google.com`, 1 from `https://www.google.bg`, 94 had no referrer and 3 came from `https://m.facebook.com`
- **THEN** the report gives Google 220, direct 94 and other 3
