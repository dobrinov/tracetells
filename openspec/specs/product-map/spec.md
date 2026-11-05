# product-map Specification

## Purpose
Shows a website's product as a map of its areas, sized by how much they are used and coloured by how often something goes wrong, and leads from an area to its features, pages and each page's story.

## Requirements

### Requirement: Areas as a map
Product map SHALL draw each area as a tile whose size follows its sessions in the window, coloured by its status:
- used, but it hurts
- little used and it hurts
- little used
- healthy
- too little data

Colours SHALL differ in lightness as well as hue, and each tile SHALL say its status in words. Without declared areas, the areas Trace suggests from the website's URLs SHALL be drawn, saying they are suggested.

#### Scenario: Practice hurts
- **WHEN** Practice has the most sessions and is "used, but it hurts", and Parents is healthy
- **THEN** Practice is the largest tile, in the strongest colour, and Parents a smaller tile in the healthy colour

### Requirement: From an area to its pages
Choosing an area SHALL open its page: its features with reach, adoption and problems, its problems, funnels and insights, and its pages. Every page path shown SHALL open that page's story.

#### Scenario: From Practice to a question
- **WHEN** someone chooses Practice and then `/questions/:id` among its pages
- **THEN** the story of `/questions/:id` opens inside Product map

### Requirement: All pages
Product map SHALL have a Pages view listing the website's pages with views, sessions, exit rate, scroll and time, each opening its story.

#### Scenario: Pages
- **WHEN** someone opens Product map › Pages
- **THEN** the pages are listed busiest first, and each opens its story
