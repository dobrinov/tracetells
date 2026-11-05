# dashboard-navigation Specification

## Purpose
Organises a website's dashboard into six places, each answering one question, reached from one sidebar, so people know where they are and where to look next.

## Requirements

### Requirement: Six places in one sidebar
A website's dashboard SHALL have six places, in this order: Pulse, Journeys, Product map, Problems, Goals and Work, plus Settings at the bottom of the sidebar. There SHALL be no second row of menus. A place's own views SHALL appear inside the place, not in the navigation.

#### Scenario: Opening a website
- **WHEN** someone opens a website
- **THEN** Pulse opens, and the sidebar shows the website switcher, the six places, Settings and the account

### Requirement: Each place has its own address
Each place SHALL have its own address: `#/pulse`, `#/journeys`, `#/map`, `#/problems`, `#/goals`, `#/work` and `#/settings/<page>`. Things inside a place SHALL extend its address: an area `#/map/<area>`, the pages `#/map/pages`, a page's story `#/map/pages/<path>`.

#### Scenario: A page's story
- **WHEN** someone opens the story of `/questions/:id` from Problems
- **THEN** the address is `#/map/pages/%2Fquestions%2F%3Aid` and the sidebar marks Product map

### Requirement: The sidebar counts what's waiting
The sidebar SHALL show beside Problems how many pages have problems in the last 7 days, beside Goals how many goals with enough data are at target out of how many, and beside Work how many insights are suggested and not yet picked. Each count SHALL be left out when it is zero.

#### Scenario: One new insight
- **WHEN** a website has one suggested insight and 2 of its 7 measurable goals at target
- **THEN** the sidebar shows "1 new" beside Work and "2/7" beside Goals

### Requirement: Navigation on a phone
At phone width the sidebar SHALL become a top bar with the website switcher and a menu, and a bottom bar with Pulse, Journeys, Problems, Goals and Work. Product map, Settings and the account SHALL be in the menu. Every control SHALL be at least 44 px.

#### Scenario: A phone
- **WHEN** someone opens a website on a 390 px wide screen
- **THEN** the bottom bar shows the five places, and the page has no sideways scrolling

### Requirement: The public view keeps its pages
A website's public link SHALL show Journeys, Product map (pages only), Problems, Goals and Work, read-only, with the sidebar. Pulse, Settings and anything that needs a signed-in person SHALL NOT appear in it.

#### Scenario: Opening a public link
- **WHEN** someone opens a website's public link
- **THEN** Work opens, and the sidebar lists only the places the public view shows
