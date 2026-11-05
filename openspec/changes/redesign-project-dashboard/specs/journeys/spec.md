# Spec Delta

## Purpose

Shows how people get through a website, step by step, where they stop, and where they arrive and leave, so a team sees which drop-off costs the most.

## ADDED Requirements

### Requirement: Funnels as step columns
Journeys SHALL show one funnel at a time, chosen from a list of all declared and recognised funnels, with each one's conversion. Each step SHALL be drawn as a column sized against the first step, showing its sessions, how many carried on from the step before and how many stopped there. The step losing the largest share SHALL be marked as the biggest drop.

#### Scenario: Sign-up to placed student
- **WHEN** 53 sessions reach the sign-up form, 28 make an account, 17 start the placement and 7 see its result
- **THEN** the columns show 53, 28, 17 and 7; the last step shows 41% carried on and 10 stopped, marked as the biggest drop

#### Scenario: A funnel without enough data
- **WHEN** a declared funnel has no sessions in the window
- **THEN** it is listed as "collecting" and can't be chosen

### Requirement: Where people arrive and leave
Journeys SHALL list the pages sessions start on and the pages they end on, with sessions, largest first, at most 10 each. Pages from fewer than 5 sessions SHALL be counted together as "other". For each arrival page it SHALL say how many of those sessions saw no second page.

#### Scenario: A landing page from search
- **WHEN** 98 sessions start on a competition page and 79 of them see no other page
- **THEN** it leads the arrivals with 98, noting that 79 left without a second page
