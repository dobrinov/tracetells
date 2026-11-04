# Spec Delta

## Purpose
Dead clicks and rage clicks tell a team where visitors press something and the page doesn't answer. They must count only clicks the page really ignored, or insights send teams to fix controls that work.

## ADDED Requirements

### Requirement: A dead click is a press the page ignored
The collector SHALL record a dead click when nothing in the page changed, nothing scrolled and the address stayed the same from the moment the visitor pressed the element until one second after the click. A change made between the press (pointer or touch down) and the click SHALL count as the page reacting. Links that open elsewhere, form fields and labels SHALL NOT be judged.

#### Scenario: A control that reacts on press
- **WHEN** a visitor clicks a button whose handler changes it on pointer down and does nothing on click
- **THEN** no dead click is recorded

#### Scenario: A control that does nothing
- **WHEN** a visitor clicks a plain element with no handler and nothing on the page changes for a second
- **THEN** one dead click is recorded for that element

#### Scenario: A keyboard click
- **WHEN** a visitor activates a button with Enter or Space and nothing changes for a second
- **THEN** one dead click is recorded, as before

### Requirement: Drawing surfaces are not judged dead
The collector SHALL NOT record dead clicks on `<canvas>` elements, whose response to a click is drawn in pixels the page structure doesn't show.

#### Scenario: Drawing on a scratch pad
- **WHEN** a visitor draws on a canvas scratch pad
- **THEN** no dead click is recorded for the canvas

### Requirement: Clicks on the page itself and text selections are not judged
The collector SHALL NOT record a dead click for a click that lands on no element but the page itself (`html` or `body`), nor for a click after which, one second later, text is selected in the page. Whether text is selected SHALL be the only thing noted, never the text itself.

#### Scenario: Clicking empty space
- **WHEN** a visitor clicks an empty margin of the page and nothing changes
- **THEN** no dead click is recorded

#### Scenario: Selecting a sentence
- **WHEN** a visitor drags across a paragraph to select a sentence, or double-clicks a word
- **THEN** no dead click is recorded for the paragraph

#### Scenario: A plain element that is not the page
- **WHEN** a visitor clicks an image with no handler, selects nothing, and nothing changes for a second
- **THEN** one dead click is recorded for the image, as before

### Requirement: A rage click is a burst the page ignored
The collector SHALL record a rage click when three or more clicks land within 700 ms and 40 px of each other and the page did not react, as defined for dead clicks, from the first press of the burst until one second after its third click. A burst the page reacted to SHALL NOT be recorded as a rage click.

#### Scenario: Typing on a number pad
- **WHEN** a visitor taps the same number pad key three times within 700 ms and each tap updates the number shown
- **THEN** no rage click is recorded

#### Scenario: Hammering a broken button
- **WHEN** a visitor clicks a button three times within 700 ms and nothing on the page changes
- **THEN** one rage click is recorded for it, along with the dead click

### Requirement: Rage clicks only where dead clicks are judged
The collector SHALL NOT record a rage click for a burst on something not judged for dead clicks (form fields, labels, links that open elsewhere, the page itself, drawing surfaces), nor for a burst that leaves text selected.

#### Scenario: Triple-clicking to select
- **WHEN** a visitor triple-clicks a paragraph to select it, or triple-clicks the text in an input
- **THEN** no rage click is recorded
