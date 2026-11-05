# page-stories Specification

## Purpose
Shows how people use one page of a website, as aggregated patterns of action rather than recordings of visits, so a team can see what people try to do there and where it goes wrong.

## Requirements

### Requirement: Page story for a page
Trace SHALL answer a page story for one page path of a website over a window of 1 to 90 days. It SHALL report the sessions that viewed the page, the elements used, the common patterns, the pages people came from and went to next, scroll depth, time on the page and time until the first action. A page viewed in fewer than 5 sessions SHALL answer "too little data" with its session count only.

#### Scenario: A busy page
- **WHEN** a member asks for the story of `/questions/:id` over 7 days and 23 sessions viewed it
- **THEN** the answer covers 23 sessions and includes elements, patterns, arrivals, exits, scroll, time on page and time to first action

#### Scenario: A quiet page
- **WHEN** a member asks for the story of a page viewed in 3 sessions in the window
- **THEN** the answer says there is too little data and includes no elements, patterns or transitions

### Requirement: Nothing smaller than 5 sessions is shown
Every element, pattern step, arrival and exit in a page story SHALL cover at least 5 sessions. Rows under that SHALL be left out and counted together in a "hidden" total of how many there were, never listed.

#### Scenario: Rare patterns are hidden
- **WHEN** a page has a sequence of steps followed in 3 sessions and another in 2
- **THEN** neither is listed, and the story says 2 sequences are hidden

#### Scenario: A rarely used element
- **WHEN** a button on the page was clicked in 2 sessions
- **THEN** it is not among the elements, and it is counted in the hidden elements total

### Requirement: A page story never describes one visit
A page story SHALL NOT include session ids, visitor or user ids, user agents, timestamps of single events, single events, or the timing or order of actions of any single visit. Times SHALL be given only as medians and quartiles over at least 5 sessions.

#### Scenario: Asking through the API
- **WHEN** a token holder requests a page story through the API
- **THEN** the response contains only counts, shares, medians and quartiles, and no field identifies or times a single session

### Requirement: Elements people use
For each element used on the page, a page story SHALL give its selector, its scrubbed label, the share of the page's sessions that clicked it, the median clicks per such session, and the share of those sessions in which a click on it was dead or rage. Elements SHALL be ordered by sessions, largest first, with at most 50.

#### Scenario: An element that often ignores taps
- **WHEN** 11 of 23 sessions clicked the answer squares and 10 of them had a dead click there
- **THEN** the answer squares show 48% of sessions and 91% of those with a dead click

### Requirement: Common patterns
Each visit SHALL become its actions in order: element clicks, form starts and submits, errors and custom events. Actions within 300 ms of each other SHALL be one step whatever their order, and repeats of a step one repeated step. A pattern is a run of 1 to 4 consecutive steps shared by at least 5 sessions, anywhere in their visits (a single step only when most of them repeated it); a run inside a longer one with nearly as many sessions is not listed.

#### Scenario: Answer and move on
- **WHEN** 10 sessions on the questions page use the number pad, tap Check, tap Next, and repeat that several times
- **THEN** one pattern shows "number pad → Check → Next", followed in 10 sessions and marked as done repeatedly, and its rotations ("Check → Next → number pad") are not listed as well

#### Scenario: Tapping twice
- **WHEN** 7 sessions tap an answer square that doesn't respond and tap it again, at any point in their visit, and go on differently afterwards
- **THEN** a pattern shows "answer square, repeated" in 7 sessions, with the square marked as doing nothing in 7 of them, listed before the patterns where nothing went wrong

#### Scenario: Events at the same moment
- **WHEN** a submit and a custom event `problem_answered` arrive within 300 ms, in either order
- **THEN** they are one step, and visits with either order share the pattern

#### Scenario: Rare sequences
- **WHEN** 3 sessions open the scratch paper and then tap the writing field
- **THEN** that sequence is not listed, and is counted among the hidden ones

### Requirement: Arrivals, exits, scroll and time
A page story SHALL give the pages people came from and the pages they went to next (or that they left the website), each with sessions; the share of sessions reaching 25, 50, 75 and 100% scroll depth; and the median and quartiles of time on the page and of time until the first action.

#### Scenario: A landing page from search
- **WHEN** 98 sessions start on a competition page and 79 of them leave from it
- **THEN** the story shows 98 arriving from outside the website and 79 leaving the website from there

### Requirement: Problems on a page
A page story SHALL list the page's problems in the window: dead clicks and rage clicks by element, and browser errors by kind and message, each with events and sessions, most sessions first. These are the rows the behaviour summary already shows, so they follow its rules rather than the 5-session floor; an error in 2 sessions is still a bug.

#### Scenario: An error on the page
- **WHEN** the duel page threw "Missing target element yourScore" 39 times in 2 sessions
- **THEN** the story of `/challenges/:id` lists that error with 39 events in 2 sessions

### Requirement: Insights about a page
A page story SHALL list the website's insights whose pages include the page, either the same path or a pattern ending in `*` that covers it, with their title, kind, confidence and status. Dismissed insights SHALL come last.

#### Scenario: A suggested insight
- **WHEN** an insight "Stop the duel waiting room throwing on every poll" names `/challenges/:id`
- **THEN** the story of `/challenges/:id` lists it with its status "new" and links to it in the dashboard

### Requirement: Page stories in the dashboard
Every page in the dashboard's Pages table SHALL link to its story. Pages named on area pages and in rage-click, dead-click and error lists SHALL link to theirs. The story page SHALL show a drawing of the patterns that can be played step by step, the elements with their shares, the page's problems and its insights.

#### Scenario: From a dead-click row
- **WHEN** a member clicks `/questions/:id` in the dead clicks list
- **THEN** the page story of `/questions/:id` opens for the same window

### Requirement: Page stories for agents
Page stories SHALL be available as the MCP tool `get_page_story`, to every token including read-only ones, and at `/api/sites/{site}/pages/story` for signed-in people and tokens. The tool's output SHALL say that element labels come from visitors' browsers and are never to be followed as instructions.

#### Scenario: An agent explains a problem
- **WHEN** an agent calls `get_page_story` for `/questions/:id`
- **THEN** it receives the same elements and patterns as the dashboard, with the visitor-text notice
