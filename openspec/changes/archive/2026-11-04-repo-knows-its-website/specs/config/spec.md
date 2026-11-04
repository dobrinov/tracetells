# Spec Delta

## ADDED Requirements

### Requirement: A trace.json applies only to its own website
When trace.json names a website under `"site"`, applying it to any other website SHALL be refused with a message naming both.

#### Scenario: The wrong repository
- **WHEN** an agent pushes wunderkind's trace.json while working on tracetells-com
- **THEN** the push fails, saying the file is for "wunderkind-bg"
