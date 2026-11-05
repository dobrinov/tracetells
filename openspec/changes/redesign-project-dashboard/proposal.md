# Proposal

Kind: feature (a redesign of the dashboard's information architecture and main screens, following the design canvas "Trace project dashboard redesign").

## Why

Opening a website today drops you on a list of sentences, with the data spread over 14 pages in three menus (Understand, Improve, Setup) plus Home. The same problem shows up on Features, Pages and as an insight, each worded differently, and Insights and Releases are separate pages, so one change can't be followed from idea to result. Nothing on the first screen shows at a glance whether the website is healthy. Teams get lost.

## What Changes

The dashboard is reorganised into **six places in one sidebar**, each answering one question, and a project opens on the first:

| Place | Answers | Replaces |
|---|---|---|
| **Pulse** | Is the website OK, what changed, what's next? | Home, Audience |
| **Journeys** | Where do people get through, and where do they drop out? | Funnels |
| **Product map** | Which parts get used, and which hurt? | Features, Pages, page stories |
| **Problems** | What is going wrong for people right now? | Errors, the frustration tables on Pages |
| **Goals** | Are the numbers we care about at target? | Goals |
| **Work** | What are we changing, and did it help? | Insights, Releases |

Settings (Get started, Install and tracking, Privacy, Team, Plan) moves to the bottom of the sidebar. On a phone the sidebar becomes a bottom bar.

It ships in **five stages**, each usable on its own:

1. **Navigation and Pulse.** The sidebar, the new addresses (old links redirect), and Pulse:
   - a one-sentence summary
   - traffic tiles with a daily chart, where sessions come from, phones and leaving after one page
   - every goal drawn as a bar against its target line, furthest behind first
   - "Next up": the three most important things to do
   - the funnels and the product map in miniature

   The other places show today's pages under their new names.
2. **Problems.** One inbox: a table of pages by dead clicks, rage clicks, browser errors and sessions hit, shaded by how much, with a detail panel and the way to hand each problem to the coding agent. Server errors are listed with it.
3. **Journeys.** Funnels drawn as step columns with how many carried on and stopped at each, the biggest drop marked, and where people arrive and where they leave.
4. **Work.** Insights and releases on one board: Suggested, Picked, In progress, Released (measuring), Measured (with the verdict).
5. **Product map.** Areas drawn as a map, sized by use and coloured by how often something goes wrong, then an area's features and pages, then each page's story.

Where people come from (the referrer's origin) becomes part of the audience report, which Pulse shows.

The dashboard still changes nothing an agent should change. The Work board has no buttons that move insights: each card shows the command to give the agent, as cards do today.

## Capabilities

### New Capabilities
- `dashboard-navigation`: the six places, their addresses, the sidebar and phone bar, its counts, and which places the public view shows.
- `problems-inbox`: one place for a website's dead clicks, rage clicks, browser and server errors, by page.
- `journeys`: funnels as step columns, and where sessions arrive and leave.
- `work-board`: insights by stage, from suggested to measured.
- `product-map`: areas and features as a map, their pages, and each page's story.

### Modified Capabilities
- `dashboard`: old links (from both earlier layouts) open the place that replaced them.
- `website-home`: the website's home becomes Pulse, which keeps the ranked "what needs attention" list as Next up and adds the website's state at a glance.
- `audience`: the report gains where sessions come from, by referrer origin.

## Privacy impact

- **Collected:** nothing new.
- **Stored:** nothing new. Sessions already store the referrer's origin, never a full URL.
- **Exposed:** where sessions come from, as shares by referrer origin. Origins from fewer than 5 sessions are counted together as "other". Every other number is an aggregate the dashboard shows today, arranged differently. Arrivals and exits on Journeys follow the same 5-session rule. Dashboard self-measurement keeps naming pages by place only (`/pulse`, `/map/story`), never ids, paths or website names.

## Site impact

None: the collector is unchanged.

## Impact

- **Dashboard** (`crates/trace-server/assets/`):
  - `index.html`: the sidebar replaces the header nav and subnav.
  - `style.css`: layout, sidebar and the new components.
  - `app/core.js`: places, addresses and redirects.
  - `app/boot.js`: routes.
  - New scripts `app/pulse.js`, `app/problems.js`, `app/journeys.js`, `app/work.js` and `app/map.js`, each added to `APP_SCRIPTS` (api.rs) and to `index.html`.
  - Existing page scripts are reused where a place shows today's page.
- **trace-core:**
  - `audience.rs`: sources by referrer origin.
  - A per-page problems aggregate, next to `digest.rs`.
  - Arrivals and exits, next to `funnel.rs`.
- **trace-server:** `GET /api/sites/{site}/problems` and `GET /api/sites/{site}/journeys`.
- **Other surfaces:**
  - `today.rs` links point at the new addresses.
  - The public view keeps its pages under the new names.
  - README and docs say where things are.
