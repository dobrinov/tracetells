# Spec Delta

## Purpose

A per-website, read-only public link that lets anyone see a website's aggregated analytics without signing in, while keeping settings, people, secrets and anything editable private.

## ADDED Requirements

### Requirement: Switching the public view on and off
Owners and admins SHALL be able to switch on a public view for one website, which gives it a public link containing a random, unguessable token. They SHALL be able to switch it off, or replace the link with a new one. Turning it off or replacing it SHALL make the old link stop working immediately. Each of these actions SHALL be recorded in the account's activity log. A website's public view SHALL be off until someone switches it on.

#### Scenario: An admin publishes a website
- **WHEN** an admin switches on the public view for the website "stride"
- **THEN** the Setup page shows its public link, the activity log shows who switched it on, and the account's other websites stay private

#### Scenario: A user can't publish
- **WHEN** a person with the user role tries to switch on the public view
- **THEN** the request is refused and the website stays private

#### Scenario: Replacing the link
- **WHEN** an admin replaces a website's public link
- **THEN** the old link answers "not found" and the new link shows the website

### Requirement: What the public view shows
A public link SHALL show, without signing in, the website's name, whether it is collecting, its KPIs against their targets, the behaviour digest, errors, insights with their statuses, each insight's prompt, the evaluations of released insights and the history of analysis runs. Everything SHALL be read-only: the public view SHALL offer no action that changes data and the server SHALL refuse every write through it.

#### Scenario: A visitor opens the shop's public link
- **WHEN** someone who is not signed in opens the public link of the website "stride"
- **THEN** they see its KPIs, behaviour, errors and insights, and they can copy an insight's prompt

#### Scenario: Writes are refused
- **WHEN** a request through a public link tries to change an insight's status, run analysis, push a `trace.json` or add a privacy rule
- **THEN** it is refused and nothing changes

### Requirement: What the public view hides
The public view SHALL NOT reveal the website's write key, privacy rules, people, roles, invites, API tokens, the activity log, the plan or its usage, the account's other websites, or which person changed an insight's status. A public link SHALL give no access to any other website, even one in the same account.

#### Scenario: Secrets stay private
- **WHEN** a visitor opens a public link
- **THEN** no response contains the write key, an API token, an email address or a privacy rule

#### Scenario: One link, one website
- **WHEN** a visitor uses the public link of "stride" to ask for another website's data
- **THEN** the request answers "not found"

### Requirement: Small numbers are suppressed
In the public view, every aggregate built from fewer than 5 sessions SHALL be left out: pages, routes, clicks, errors, form steps and funnel steps. Totals SHALL stay accurate. The view SHALL say that rows from fewer than 5 sessions are hidden.

#### Scenario: A rare path stays hidden
- **WHEN** only 2 sessions in the period visited `/help`
- **THEN** `/help` appears for signed-in members of the account but not in the public view

### Requirement: The public view is labelled
The public view SHALL say that it is a public, read-only view of one website, link to Trace, and offer no sign-in-only navigation such as Setup, Privacy rules, Team, Plan or Account.

#### Scenario: A visitor knows where they are
- **WHEN** a visitor opens a public link
- **THEN** the page is labelled as a public read-only view of that website, and Insights, KPIs and Behaviour are the only sections offered
