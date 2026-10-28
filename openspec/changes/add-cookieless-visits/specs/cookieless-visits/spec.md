# Spec Delta

## Purpose

Measure visits without storing anything on visitors' devices.

## ADDED Requirements

### Requirement: Nothing stored on the device by default
Without a consent setting, the collector SHALL NOT write to cookies, `localStorage` or `sessionStorage`, and SHALL send no visitor or session id.

#### Scenario: A visit to a default install
- **WHEN** a visitor opens two pages of a site using the default snippet
- **THEN** their browser holds nothing from Trace, and Trace records one session with both pages

### Requirement: Visits recognised on the server
For a batch without a session id, the server SHALL derive the visitor from the website, address and browser with a daily salt, continue that visitor's session if it was active in the last 30 minutes, and never store the address. Salts older than yesterday SHALL be deleted.

#### Scenario: Another browser on the same network
- **WHEN** two browsers at the same address visit the site
- **THEN** they are two sessions

#### Scenario: The next day
- **WHEN** the same visitor comes back the next day
- **THEN** their visitor id is different and can't be linked to the day before

### Requirement: Consent from the site's own tool
With `consent: required`, the collector SHALL send nothing until `trace.consent(true)`, and SHALL show nothing itself.

#### Scenario: Waiting for the consent tool
- **WHEN** a site uses `data-consent="required"` and the visitor hasn't answered
- **THEN** no request reaches `/v1/collect`
