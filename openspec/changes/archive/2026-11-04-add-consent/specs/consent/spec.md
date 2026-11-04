# Spec Delta

## Purpose

Let a site ask its visitors before Trace stores or sends anything, and respect the answer.

## ADDED Requirements

### Requirement: Nothing before consent
With `consent` set to `banner` or `required`, the collector SHALL NOT store identifiers, listen to interactions or send events until the visitor agrees. Calls made meanwhile SHALL wait, and SHALL run once the visitor agrees.

#### Scenario: A first visit
- **WHEN** a visitor opens a page with `data-consent="banner"` and clicks around without answering
- **THEN** no visitor or session id is stored and no request goes to the collect endpoint

#### Scenario: The visitor allows
- **WHEN** the visitor clicks Allow
- **THEN** collection starts on that page, and on later visits it starts without asking

### Requirement: Saying no is respected
No thanks, `trace.consent(false)` or Global Privacy Control SHALL keep the collector from storing or sending anything. Saying no after agreeing SHALL delete Trace's ids and stop collection on the page.

#### Scenario: Withdrawing
- **WHEN** a visitor who allowed earlier opens the bar from a `data-trace-consent` link and clicks No thanks
- **THEN** Trace's ids are deleted, and later clicks send nothing

#### Scenario: Global Privacy Control
- **WHEN** a browser that sends Global Privacy Control opens a page with `data-consent="banner"`
- **THEN** no bar is shown and nothing is stored or sent

### Requirement: An even-handed bar
The `banner` bar SHALL say in one sentence what Trace does and keeps, offer Allow and No thanks as equally prominent buttons, and work with the keyboard. Any element with `data-trace-consent` SHALL open it again.

#### Scenario: Changing your mind
- **WHEN** a visitor clicks a footer link with `data-trace-consent`
- **THEN** the bar opens with both choices
