# Spec Delta

## Purpose

The people who run a Diagnostic server (operators) need one internal place to see every account with its people and websites, to manage plan templates and per-account limits, and to create accounts, without being able to read any account's analytics.

## ADDED Requirements

### Requirement: Granting operator rights
Operator rights SHALL be granted and removed only from the server's command line, for a person who already has an account. No web page or API request SHALL be able to grant operator rights. Every grant and removal SHALL be recorded.

#### Scenario: Granting from the command line
- **WHEN** someone on the server runs `diagnostic operator grant owner@example.com`
- **THEN** that person has operator rights the next time they sign in, and the change is recorded

#### Scenario: Trying to grant over the API
- **WHEN** any request to the HTTP API or MCP tries to give a person operator rights
- **THEN** no operator rights are granted

### Requirement: Operators use two-step verification
A person with operator rights SHALL only be able to use the operator area after signing in with two-step verification. Without it, the operator area SHALL ask them to set it up first. Their normal account access SHALL be unaffected.

#### Scenario: An operator without two-step verification
- **WHEN** a person with operator rights who has not set up two-step verification opens the operator area
- **THEN** they are asked to set it up, and no operator data is shown until they have

### Requirement: Seeing accounts, people and websites
The operator area SHALL list every account with its plan, effective limits, overrides and this month's usage. For each account it SHALL show the people (email, name, role, two-step status, last sign-in) and websites (name, identifier, collecting addresses, collection status). It SHALL be visible only to operators.

#### Scenario: Browsing accounts
- **WHEN** an operator opens the operator area
- **THEN** they see every account on the server with its plan and usage, and can open one to see its people and websites

#### Scenario: Someone who isn't an operator
- **WHEN** an account owner who is not an operator requests the operator area or its API
- **THEN** it is not shown and the request is refused

### Requirement: Operators don't see analytics
The operator area SHALL NOT show any account's behaviour data, KPIs, ideas, prompts or privacy-rule contents. Operator rights SHALL grant no access to other accounts' websites in the dashboard, API or MCP.

#### Scenario: Looking at a customer's website
- **WHEN** an operator opens a customer account's website in the operator area
- **THEN** they see its name, identifier, collection status and usage counts, but no behaviour data, KPIs or ideas

### Requirement: Managing plans and accounts
Operators SHALL be able to edit plan templates and choose the default template. They SHALL be able to move an account to another template, set or remove per-account overrides, and create accounts. Every such change SHALL be recorded with the operator who made it.

#### Scenario: Moving a friend to the friend plan
- **WHEN** an operator moves the account "Bolt" from `free` to `friend`
- **THEN** Bolt's effective limits are `friend`'s from then on, apart from any overrides, and the change is recorded
