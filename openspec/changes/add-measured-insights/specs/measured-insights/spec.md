# Spec Delta

## Purpose

Insights are grounded in data Trace measured, shown to the person before they're saved, and say who made them, when, and from what.

## ADDED Requirements

### Requirement: Measured evidence
Every evidence item of an insight SHALL be one of:
- a **measurement**: a claim, a query (as `query_behaviour` takes it, without a breakdown), and which way the number should move if the problem is fixed (`decrease` or `increase`). Trace runs the query itself and stores the value, the session count and the window.
- a **note**: text only, shown as "not measured".

An insight SHALL have at least one measurement. Wherever a measurement is shown, the number SHALL be Trace's, not one the agent typed.

#### Scenario: Only notes
- **WHEN** an agent drafts an insight whose evidence is all notes
- **THEN** the draft is refused, saying at least one item must be a measurement

#### Scenario: A measurement
- **WHEN** an agent's evidence says "Most checkout visitors rage-click Pay now", with the query for that rate
- **THEN** the draft shows the claim with Trace's measured rate and the number of sessions behind it

### Requirement: Drafts
Drafting SHALL validate each proposed insight, measure its evidence, and return each draft with its measured numbers and a draft id, or explain what is wrong with it. Drafts SHALL NOT appear as insights anywhere and SHALL expire after one hour. Saving SHALL need a token that isn't read-only; drafting SHALL NOT.

#### Scenario: A read-only agent drafts
- **WHEN** an agent connected read-only drafts an insight and then tries to save it
- **THEN** it gets the draft with Trace's numbers, and saving is refused as read-only

### Requirement: Draft, show, then save
Insights from agents SHALL be created in two steps: drafting, then saving. Saving SHALL take draft ids and save only those drafts, unchanged. The MCP server's instructions and the analysis prompt SHALL tell agents to show the drafts to the person and save only those the person approves.

#### Scenario: The person approves two of three
- **WHEN** an agent drafts three insights, shows them, and the person approves the first and the third
- **THEN** the agent saves those two draft ids, and only those two insights exist

#### Scenario: A stale draft
- **WHEN** an agent tries to save a draft created more than an hour ago
- **THEN** saving refuses it and says to draft again

#### Scenario: Something Trace already has
- **WHEN** a draft matches an existing insight in any status, including dismissed, by fingerprint
- **THEN** drafting says so instead of returning a draft

### Requirement: Provenance
Every insight SHALL record:
- **who** created it: the person, with the app or token they used (for example "deyan@example.com via Claude Code (signed in 2026-10-28)"), or "Trace's built-in checks", or "Trace's Claude analyst";
- **when** it was created;
- **the window** its evidence was first measured over.

The dashboard, the API, MCP and the insight's task prompt SHALL show this. The public view SHALL show what created it (an agent, the built-in checks or Claude) and when, but not the person.

#### Scenario: In the task prompt
- **WHEN** someone copies an insight as a prompt
- **THEN** the prompt says who found it, when, and over which dates its evidence was measured

### Requirement: A clear choice of access
The screen where a person approves an MCP client SHALL offer two choices, each saying what the agent can do with it:
- **Full access**, selected by default: ask questions, suggest insights and save the approved ones, and set up tracking, KPIs and privacy rules.
- **Read only**: ask questions and suggest insights, but never save or change anything.

#### Scenario: Approving without changing anything
- **WHEN** a person opens the Connect screen and clicks Allow straight away
- **THEN** the agent gets full access

#### Scenario: Choosing read-only
- **WHEN** a person picks Read only
- **THEN** the screen has said the agent can ask questions and suggest insights but not save or change anything, and the agent's token is read-only
