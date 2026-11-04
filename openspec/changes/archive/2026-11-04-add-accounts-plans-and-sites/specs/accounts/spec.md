# Spec Delta

## Purpose

Accounts are the unit a company or a single person signs up as: an account owns its websites, its people and their roles, its sign-in policy, its API tokens, its activity log and its plan.

## ADDED Requirements

### Requirement: Accounts own people and settings
Every person, invite, API token, two-step verification policy and activity log entry SHALL belong to exactly one account. A person SHALL only see and act on the websites and data of their own account.

#### Scenario: Two accounts on one server
- **WHEN** Ana belongs to account Acme and Ben belongs to account Bolt
- **THEN** Ana sees only Acme's websites, people, tokens and activity, and requests Ana makes for Bolt's websites are refused as not found

#### Scenario: Upgrading an existing installation
- **WHEN** a server that ran an earlier version with one workspace is upgraded
- **THEN** that workspace becomes an account with one website, keeping its people, roles, tokens, settings, data and collector write key

### Requirement: Account roles
Each person in an account SHALL have exactly one role: `owner`, `admin` or `user`. An account SHALL have exactly one owner. Users SHALL be able to read the account's websites and work with ideas and analysis. Admins SHALL additionally manage people, invites, the two-step policy, privacy rules, websites and all of the account's API tokens. The owner SHALL additionally be the only person who cannot be removed or demoted.

#### Scenario: A user cannot manage the team
- **WHEN** a person with the `user` role opens the team settings or invites someone
- **THEN** the request is refused because it needs the `admin` role

#### Scenario: Former viewers and members after the upgrade
- **WHEN** an installation with `member` and `viewer` people is upgraded
- **THEN** all of them have the `user` role, and the account's activity log records each viewer's change of role

### Requirement: Read-only API tokens
A person SHALL be able to create a personal API token that is read-only. A read-only token SHALL be able to read everything its owner can read in the account and SHALL be refused for every change.

#### Scenario: An agent with a read-only token
- **WHEN** an agent uses a read-only token to mark an idea as released
- **THEN** the change is refused and the idea keeps its status

### Requirement: Unique sign-in email
An email address SHALL identify at most one person on a server, so that signing in never has to ask which account is meant.

#### Scenario: Inviting someone who already has an account
- **WHEN** an admin of Acme invites an email address that already belongs to a person in another account
- **THEN** the invite is refused with a message that the address is already in use

### Requirement: Sign-up and account creation
A server SHALL allow self-service sign-up only when the operator has enabled it. Sign-up SHALL create a new account on the server's default plan, with the person who signed up as its owner. An operator SHALL be able to create an account on a chosen plan and receive an invite link for its future owner.

#### Scenario: Sign-up disabled
- **WHEN** sign-up is not enabled and someone opens the sign-up page
- **THEN** they are told that accounts are created by invitation, and no account is created

#### Scenario: Sign-up on a hosted server
- **WHEN** sign-up is enabled, the default plan is `free`, and a visitor signs up with a new email address
- **THEN** a new account on the `free` plan exists with that person as its owner, and they are taken to setting up their first website

#### Scenario: Operator creates a friend's account
- **WHEN** an operator creates an account named "Bolt" on the `friend` plan for bob@bolt.example
- **THEN** the operator receives a single-use invite link that makes Bob the owner of Bolt when he accepts it
