# Spec Delta

## ADDED Requirements

### Requirement: What needs attention, ranked
A website's home SHALL list what needs attention now, most important first, each with a link to where it is acted on; unfinished setup SHALL come first while the website collects nothing.

#### Scenario: A new website
- **WHEN** a website has no data yet
- **THEN** its home's first item is the next setup step

#### Scenario: Switching websites
- **WHEN** someone on one website's Get started page switches to another website
- **THEN** the other website opens on its home
