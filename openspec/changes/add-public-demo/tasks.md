# Tasks

## 1. Storage and suppression

- [x] 1.1 Add `public_token` (unique), `public_since` and `public_by` to `projects` (schema and `db::migrate`), with functions to switch the view on, rotate it, switch it off and look a website up by token
- [x] 1.2 Add `digest::public(digest, min_sessions)` dropping rows below the threshold from every list (by `count` for transitions) and recording `suppressed_below`, with a unit test covering each list and unchanged totals

## 2. API

- [x] 2.1 Add the admin routes `POST /api/sites/{slug}/public`, `POST …/public/rotate` and `DELETE …/public`, writing `site.public_on`, `site.public_rotated` and `site.public_off` to the activity log
- [x] 2.2 Add the unauthenticated GET-only router `/api/public/{token}/…` (overview, kpis, digest, insights, insight, prompt, runs) with allowlisted responses: no write key, slug, Claude settings, `dismiss_reason` or who changed a status
- [x] 2.3 Serve the dashboard at `/p/{token}`
- [x] 2.4 Test the routes:
  - a user can't switch the view on;
  - an unknown or rotated token answers 404;
  - writes through the public prefix are refused;
  - a public token can't reach another website;
  - no public response contains the write key, an API token, an email address or a privacy rule;
  - rare rows are suppressed in the public digest but not in the signed-in one

## 3. Dashboard

- [x] 3.1 Add the public mode to `app.js`:
  - detect `/p/{token}` and route every call through one `apiBase()` helper;
  - offer only Insights, KPIs and Behaviour, with no write controls;
  - show the "Public, read-only view of <website>" banner and the note about hidden rows
- [x] 3.2 Add the "Public view" card to the Setup page: Make public, Copy link, Replace link and Make private for admins, read-only for users
- [x] 3.3 Verify in the browser: switch a website public, open its link signed out, check that nothing editable shows, rotate the link and see the old one stop working

## 4. Shop on two hosts

- [x] 4.1 Give `shop.js` a base path (`data-base`) for links, routing and `data-routes`, and a static reviews mode (`data-reviews="static"`) that asks for a missing file 3 times in 10
- [x] 4.2 Keep `trace serve --demo-site` working at the root; add a test that the line `demo.rs` names for the Annual error still throws on `pricing.yearly`
- [x] 4.3 Make the shop's demo bar link to `{{PUBLIC_URL}}` (in `trace serve --demo-site`, the local dashboard's Behaviour page as today)

## 5. Marketing site and deployment

- [x] 5.1 Add the collector snippet, `data-trace-event="start_free"` and `"open_demo"` attributes, "Try the demo shop" and "See this page's own analytics" links, and `site/trace.json` with the two KPIs
- [x] 5.2 Extend `pages.yml`:
  - copy the shop into `_site/demo/` and write the review files and `404.html`;
  - substitute the base path, server address, write keys and public links from repository variables;
  - drop the collector when `TRACE_URL` is empty;
  - push both `trace.json` files when `TRACE_TOKEN` is set;
  - also trigger on shop changes
- [x] 5.3 Verify a local build of `_site/` (served from a sub-path): deep links open the right page, every planted problem happens, and with no `TRACE_URL` no collector loads and the placeholders show
- [x] 5.4 Run `actionlint` on the workflow
- [ ] 5.5 Run an accessibility check on the changed pages (the hero demo line, the footer links, the public view banner)

## 6. Docs

- [x] 6.1 README: the public view (what it shows and hides, suppression, rotation) and the live demo (repository variables, deploying before hosting)
- [x] 6.2 Record the open consent question (visitor id in `localStorage` on the public marketing site) for a decision before going live

## 7. Changed while building (recorded here, see proposal)

- [x] 7.1 No placeholders or repository variables: the hosted server (app.tracetells.com) exists, so the write key and public links are in `pages.yml`; the build lives in `deploy/build-site.sh` so it can be run locally
- [x] 7.2 Public endpoints are `/v1/public/{token}/…` (outside `/api`, which is signed-in), and the admin routes are `/api/sites/{site}/publish` so they can't collide with them
- [x] 7.3 The public view is also controllable over MCP (`public_view`) and the CLI (`trace site publish`); `trace operator set-plan` added to put the operator's own account on the Owner plan
- [x] 7.4 Consent: the shop uses the consent bar like the rest of tracetells.com (the open question is settled by `add-consent`); the privacy policy says this site's aggregated analytics are public
- [x] 7.5 Transitions count sessions, so suppression applies to them by session
