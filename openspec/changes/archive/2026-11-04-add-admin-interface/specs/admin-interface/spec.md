# Spec Delta

## ADDED Requirements

### Requirement: Viewing as someone is read-only and visible
An operator viewing as a person SHALL only be able to read, ask questions and switch between that person's accounts; it SHALL end after an hour; it SHALL be refused for another operator; and it SHALL be recorded in the viewed account's activity log.

#### Scenario: Trying to change something
- **WHEN** an operator viewing as a customer sends an invite
- **THEN** it is refused as read-only

#### Scenario: The customer can tell
- **WHEN** an operator views as someone in an account
- **THEN** that account's activity log shows `operator.viewed_as` with the operator's email
