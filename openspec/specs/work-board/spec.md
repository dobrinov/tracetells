# work-board Specification

## Purpose
Follows every change a team makes from the insight that suggested it to whether it helped, on one board, so nobody has to piece the story together from separate lists.

## Requirements

### Requirement: Insights by stage
Work SHALL show the website's insights as cards in five columns, in order:
- **Suggested**: new
- **Picked**: selected
- **In progress**
- **Released, measuring**: released, with no settled verdict yet
- **Measured**: released, with a settled verdict

Dismissed insights SHALL be hidden behind a "Dismissed" toggle that shows each one's reason. On a narrow screen the columns SHALL scroll sideways inside the board, not the page.

#### Scenario: One suggestion
- **WHEN** a website has one new insight and nothing else
- **THEN** Suggested holds its card, and each other column says in one line what goes there

### Requirement: What a card shows
Every card SHALL show its title, whether it is a pattern or a defect, its confidence, its pages and the goal it should move, and SHALL open the insight in full. A card being measured SHALL show how far through its 14 days it is. A measured card SHALL show its goal before and after the release and the verdict.

#### Scenario: Day 6 of 14
- **WHEN** an insight was released 6 days ago
- **THEN** its card shows "measuring, day 6 of 14"

### Requirement: The board changes nothing
The board SHALL NOT move, pick, dismiss or release insights. Each card SHALL show the command to give the coding agent for its next step (for a suggested one, picking it; for a picked one, `/trace:fix`; for one in progress, releasing it), with Copy.

#### Scenario: Picking an insight
- **WHEN** someone wants to pick a suggested insight
- **THEN** its card shows `/trace:manage pick insight <id>` to copy, and no button that changes it
