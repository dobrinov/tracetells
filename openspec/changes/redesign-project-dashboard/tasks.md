# Tasks

There is no JavaScript test harness, so dashboard tasks are verified in a browser on the local demo (`trace --db demo.db demo`, then `trace --db demo.db --site stride serve --demo-site 127.0.0.1:4319`) and, where named, against a copy of wunderkind.bg's numbers. Rebuild before each browser check: the dashboard is compiled into the binary.

## 1. Stage 1: navigation and Pulse

- [x] 1.1 Replace `SECTIONS`/`ALIASES` in `app/core.js` with `PLACES` and one `REDIRECTS` table covering the flat links, the four areas and Home's bare item keys, and route `#/<place>/<view>/<arg>` in `app/boot.js`; verify in the browser that `#/kpis`, `#/home`, `#/understand/audience`, `#/understand/errors`, `#/understand/pages/%2Fcheckout`, `#/improve/releases` and `#/setup/privacy` land on the addresses in the `dashboard` spec scenarios
- [x] 1.2 Move the header's nav, site switcher, collection status and account into a sidebar in `index.html` and `style.css` (both themes), drop `#subnav`, and render the places from `PLACES` with their views inside each place's header; verify at 1440 px and in dark mode that every place opens and the sidebar marks it
- [x] 1.3 Add the phone layout (top bar with switcher and menu, bottom bar with five places, 44 px controls, no sideways scroll); verify at 390 px width in the browser
- [x] 1.4 Show the sidebar counts for Problems (from `/digest` until stage 2), Goals (`/kpis`) and Work (`/insights?status=new`), loaded after the page; verify on the demo that the counts match those pages and that a zero count is left out
- [x] 1.5 Add `sources` to the audience report in `trace-core/src/audience.rs` (origin normalising, search engines as one source, under 5 sessions as "other", direct, the window before) and show it as a table on the Audience page; verify with a unit test for the spec scenario (google.com + google.bg = Google 220, direct 94, other 3) on SQLite and Postgres
- [x] 1.6 Write `app/pulse.js`: period switch, summary sentence, traffic tiles with daily bars and sources, goals against one target line (decrease goals inverted, under 10 sessions collecting), Next up from `/today` (with the analysis panel), the funnels in miniature, and the product map in miniature (a simple tile list until stage 5); each part links to its place and fails alone; register it in `APP_SCRIPTS` and `index.html`; verify on the demo that every part draws and links correctly, and that a new website with no data shows its setup step first
- [x] 1.7 Point the places not redesigned yet at today's pages (Journeys → Funnels, Product map → Features with a Pages view and page stories, Problems → Errors, Work → Insights and Releases as two views, Settings → its five pages), keep the public view on its pages under the new names, and open public links on Work; verify a public link shows only the places in the `dashboard-navigation` spec
- [x] 1.8 Name self-measured pages `/<place>` and `/<place>/<view>` without arguments; verify by reading the `measurePage` calls for each route that no path, area key or id is sent
- [x] 1.9 Update the README's dashboard sections, the docs pages that name Understand, Improve or Setup, and the plugin skills that point at dashboard pages; verify with `grep -rn "Understand ›\|Improve ›\|Setup ›" README.md site/src plugins` returning nothing stale

## 2. Stage 2: Problems

- [x] 2.1 Add the per-page problems report in `trace-core/src/problems.rs` (dead, rage and error counts and sessions hit per path, top 50; a page's detail and the insights naming it come from its story, `story::build`); verify unit tests for the spec's questions-page scenario on SQLite and Postgres
- [x] 2.2 Serve it at `GET /api/sites/{site}/problems?days=` through the report cache (the page lists open server errors from `/errors` beside it); verify an API test for members, read-only tokens and the isolation test's route list
- [x] 2.3 Write `app/problems.js`: shaded table (lightness as well as hue), page detail with "Make it a task" and a link to the story, server errors section with the not-set-up notice; switch the sidebar count to this report; verify on the demo that the heaviest cells are darkest, the task text names page, element, counts and window, and nothing on the page changes data
- [x] 2.4 Update the README and docs where they describe the Errors page; verify the text matches the page

## 3. Stage 3: Journeys

- [x] 3.1 Add arrivals and exits to `trace-core/src/funnel.rs` (entry pages with one-page sessions, last pages, under 5 sessions as "other", 10 each) and serve `GET /api/sites/{site}/journeys?days=` with the funnels; verify unit tests for the landing-page scenario (98 arrive, 79 leave without a second page) on both databases
- [x] 3.2 Write `app/journeys.js`: funnel picker (funnels without sessions listed as collecting), step columns with carried on and stopped, biggest drop marked, arrivals and exits; verify on the demo with the checkout funnel that the numbers match the Funnels page they replace
- [x] 3.3 Update the README's Funnels section; verify it describes the new page

## 4. Stage 4: Work

- [x] 4.1 Write `app/work.js`: five columns by status and settled verdict, dismissed toggle with reasons, cards with kind, confidence, pages and goal, "measuring, day N of 14", before and after with the verdict, the next-step agent command with Copy, and the full insight in a side panel; the board scrolls sideways inside itself on narrow screens; verify on the demo (one released insight) that it sits in the right column with its verdict, and that no control changes an insight
- [x] 4.2 Redirect `#/improve/insights` and `#/improve/releases` to `#/work` at the matching column and point Home's insight items there; verify both old links
- [x] 4.3 Update the README and the coding-agent docs where they describe Insights and Releases; verify the commands shown on the board are the ones documented

## 5. Stage 5: Product map

- [x] 5.1 Write the squarified treemap in `app/map.js` (tiles sized by sessions, status in words and colour, areas under 3% grouped, too-small tiles listed under the map, suggested areas labelled); verify on the demo and with wunderkind.bg's six areas that the busiest area is the largest tile and every area is reachable
- [x] 5.2 Put the area page, the Pages view and page stories inside Product map at their new addresses, with every page path opening its story; use the treemap in miniature on Pulse; verify from Pulse → Practice → `/questions/:id` that the story opens inside Product map
- [x] 5.3 Update the README's Areas and features and Page stories sections; verify they name Product map

## 6. Integration

- [x] 6.1 After each stage, run `cargo test --workspace` on SQLite and with `TRACE_TEST_POSTGRES` set, and clippy with the CI flags; verify all pass before that stage ships
- [x] 6.2 After stage 5, walk every old address from the `dashboard` spec and every place at 1440 px and 390 px in both themes on the demo; verify none lands on a blank or wrong page

## Workflow follow-up

- Deploy each stage after review (`/opt/apps/trace/deploy.sh`, with the user's go-ahead), and archive the change after stage 5.
