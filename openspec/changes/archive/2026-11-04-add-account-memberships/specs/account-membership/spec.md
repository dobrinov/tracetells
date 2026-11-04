# Spec Delta

## Purpose

Let one person use several accounts with one login.

## ADDED Requirements

### Requirement: Membership per account
A person SHALL be able to belong to several accounts, with a role in each. Their role in one account SHALL NOT affect what they can do in another.

#### Scenario: Owner here, user there
- **WHEN** Dimitar owns account B and is a user in account A
- **THEN** he can invite people in B, and he can't invite people in A

### Requirement: Switching accounts
A signed-in person SHALL be able to switch to any account they belong to, and only to those. Sign-in SHALL open the account they used last.

#### Scenario: Switching
- **WHEN** a person in accounts A and B switches to B
- **THEN** the dashboard shows B's websites, and signing in again later opens B

#### Scenario: Not a member
- **WHEN** a person asks to switch to an account they don't belong to
- **THEN** the request is refused and the session stays in its account

### Requirement: Invites for existing logins
An admin SHALL be able to invite an email that already has a login, unless it is already a member. That person SHALL join by signing in as that email; the invite SHALL NOT set or change their password.

#### Scenario: Joining with an existing login
- **WHEN** Dimitar, who has his own account, opens an invite to account A while signed in as the invited email and presses Join
- **THEN** he is a member of A with the invited role, and still owns his own account

#### Scenario: Wrong person
- **WHEN** someone signed in as a different email opens the invite
- **THEN** joining is refused

#### Scenario: Link without a session
- **WHEN** the invite's email already has a login and nobody is signed in
- **THEN** the link offers to sign in, and choosing a password is not possible

### Requirement: Creating another account
When sign-up is open, a signed-in person SHALL be able to create a new account they own, with the same login.

#### Scenario: Invited first, own account second
- **WHEN** a person who joined account A by invite creates account B
- **THEN** they own B, B has its first website, and they are still a member of A

### Requirement: Removing and leaving
Removing a person from an account, or the person leaving it, SHALL remove only that membership and revoke their API tokens for that account. The owner SHALL NOT leave or be removed. A person with no account left SHALL be deleted.

#### Scenario: Removed from one of two
- **WHEN** an admin of A removes a person who also owns B
- **THEN** their tokens for A stop working, and they can still sign in and use B

### Requirement: Connecting an agent to one account
Connecting an MCP client SHALL issue a token for one account the person belongs to, chosen on the Connect screen when they have several.

#### Scenario: Choosing the account
- **WHEN** a person in A and B connects Claude Code and picks B
- **THEN** the token lists B's websites and none of A's
