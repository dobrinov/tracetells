# Spec Delta

## ADDED Requirements

### Requirement: Where sessions come from
The audience report SHALL give the share of sessions by where they came from: the referrer's origin, or "direct" when there was none. Search engines, and an origin's `www.` and `m.` forms, SHALL be counted as one source (google.com and google.bg are Google). Sources from fewer than 5 sessions SHALL be counted together as "other". Each share SHALL come with the window before.

#### Scenario: Search and direct
- **WHEN** 219 sessions came from `https://www.google.com`, 1 from `https://www.google.bg`, 94 had no referrer and 3 came from `https://m.facebook.com`
- **THEN** the report gives Google 220, direct 94 and other 3
