# page-overlay Specification

## Purpose
Lets a team see a page story drawn on their own live page, opened in their own browser, without Trace recording or storing anything from visitors' screens.

## Requirements

### Requirement: Overlay links
A member SHALL be able to open a page story on the website itself, signed in or with an API token that can make changes (read-only tokens cannot). Trace SHALL make an overlay link: an address on one of the origins the collector has sent from, with a token in the URL fragment. The token SHALL be random, stored only as a hash, valid for 15 minutes, limited to that website, and grant reading page stories only.

#### Scenario: Opening the overlay
- **WHEN** a member on the story of `/questions/:id` chooses "Show on my page" and enters the address `https://wunderkind.bg/questions/123`
- **THEN** that address opens in a new tab with `#trace-overlay=` and a token in its fragment

#### Scenario: An address on another website
- **WHEN** the address entered is not on an origin the collector has sent from for this website
- **THEN** Trace refuses to make the link and says which origins it knows

#### Scenario: An old link
- **WHEN** an overlay link is opened 20 minutes after it was made
- **THEN** the overlay says the link has expired and draws nothing

### Requirement: The collector stays quiet in overlay mode
When the page loads with an overlay token in its fragment, the collector SHALL send no events for that page load, SHALL remove the token from the address bar, and SHALL load the overlay script from the Trace server it belongs to. Without a token it SHALL behave exactly as before.

#### Scenario: The team looks at its own page
- **WHEN** a member opens an overlay link to the questions page and clicks around
- **THEN** no pageview, click or other event from that tab reaches Trace, and the address bar no longer shows the token

### Requirement: Drawing the story on the page
The overlay SHALL draw, over the elements matching each selector in the story, the share of sessions that used it, and in its dead-tap view the share whose clicks there did nothing. It SHALL list the story's elements that match nothing on the page as it is now. It SHALL play a pattern by moving a marker over the elements of its steps at a fixed pace.

#### Scenario: An element that moved
- **WHEN** the story has an element whose selector matches nothing on the open page
- **THEN** the overlay lists it as "not on this page now" with its share of sessions

#### Scenario: Playing a pattern
- **WHEN** the member plays "tap a square twice"
- **THEN** the marker moves to an answer square, shows that the tap did nothing, moves to it again, then to Check, one step at a time

### Requirement: The overlay shows only the page story
The overlay SHALL read page stories only, with the same minimum-sessions rule as the dashboard, and SHALL NOT change the page's content, submit its forms, or send anything about the page to Trace.

#### Scenario: Reading the story
- **WHEN** the overlay loads for the questions page
- **THEN** its only request to Trace is for that page's story, and the response is the same aggregate the dashboard shows
