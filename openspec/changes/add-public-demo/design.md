# Design

## Context

- **Dashboard and API.**
  - `crates/trace-server/src/api.rs` serves the dashboard (`/`, `/assets/app.js`, `/assets/style.css`), the collector (`/d.js`, `POST /v1/collect`, with permissive CORS so any origin can send events) and the authenticated API.
  - Site-scoped routes (`/project`, `/digest`, `/kpis`, `/insights…`, `/runs`, `/privacy…`, `/onboarding…`, `/config`) are mounted under `/api/sites/{slug}/…`. `auth_api.rs` resolves the session or token, checks `required_role` (reads need a user, unknown writes need an admin, failing closed) and injects `CurrentSite`.
  - Nothing is reachable without signing in except the collector.
- **What the endpoints return.**
  - `get_project` returns the name, slug, **write key**, collection status and Claude settings.
  - `digest::build` (`crates/trace-core/src/digest.rs`) returns:
    - totals;
    - lists with a `sessions` count (`pages`, `rage_clicks`, `dead_clicks`, `errors`, `forms`, `devices`, `custom_events`);
    - `transitions`, which has only `count`;
    - KPI snapshots.
  - `Insight` (`model.rs`) carries `dismiss_reason`, which is free text a person typed.
- **Dashboard client.** `app.js` is a hash-routed SPA (`routes = {start, insights, kpis, behaviour, privacy, setup, account, team, plan, admin}`). It renders each view from these endpoints, using `can(role)` to hide controls.
- **The demo shop.**
  - The shop lives in `crates/trace-server/assets/shop/` (`index.html`, `shop.js`, `trace.json`).
  - `demo.rs` serves it with `{{TRACE_URL}}`/`{{WRITE_KEY}}` substituted, an SPA fallback, and a real `/api/reviews/{id}` that fails 3 calls in 10.
  - `shop.js` uses root-absolute paths (`/products`, `/cart`) and `history.pushState`.
  - `demo.rs` also seeds simulated events whose selectors (`div.product-card > img`, `button#pay`, `/shop.js:315`) must keep matching the shop.
- **The marketing site.**
  - `site/` holds static `index.html`, `pricing.html` and `style.css`. `data-app-link` anchors get their href from `data-app-url`.
  - `.github/workflows/pages.yml` uploads `site/` as-is on pushes to `site/**`.

## Goals / Non-Goals

**Goals:**
- Anyone can explore one website's aggregated analytics through a link the owner controls.
- The public view cannot write, and cannot leak secrets, people or rare paths.
- One source for the shop, deployable to GitHub Pages under a sub-path and still served by `trace serve --demo-site`.
- The marketing site measures itself with Trace.

**Non-Goals:**
- Per-visitor sandboxes, a shared demo login, or making whole accounts public.
- Running analysis from the public view. Insights refresh when the owner runs analysis, by hand or on a schedule.
- Rate limiting the collector. That is a hosted-edition concern for every site, not just the demo.
- A cookieless collector mode (see Open Questions).

## Decisions

### D1. A token per website, separate unauthenticated routes
- **Columns:** `projects` gains `public_token TEXT UNIQUE` (32 random bytes, URL-safe base64), `public_since INTEGER` and `public_by TEXT`. A null token means private.
- **Admin routes** (site-scoped, admin role), each writing `site.public_on`, `site.public_off` or `site.public_rotated` to the activity log:
  - `POST /api/sites/{slug}/public` switches the view on, or returns the existing link;
  - `POST /api/sites/{slug}/public/rotate` replaces the link;
  - `DELETE /api/sites/{slug}/public` switches it off.
- **Public routes:** `GET /api/public/{token}/{overview|kpis|digest|insights|insights/{id}|insights/{id}/prompt|runs}`.
  - They sit on a router of their own, outside `auth_api`'s middleware.
  - Only GET is mounted, so a write can't reach a handler (writes answer 405).
  - The router looks the token up, rejects unknown ones with 404, and injects `CurrentSite` plus a `PublicView` marker.
  - Handlers are shared with the signed-in API where the output is safe. Each public handler builds its response from an explicit allowlist rather than stripping fields, so a field added later stays private by default.
- **Overview response:** name, collecting yes or no, last event time. No slug, write key or Claude settings.
- **Insight response:** the insight without `dismiss_reason` and without who changed its status.
- **Alternative considered:** a `public` flag plus the site slug. Rejected because slugs are guessable and couldn't be rotated.

### D2. Small-number suppression on the digest
- `digest::public(digest, min_sessions = 5)` filters each list:
  - lists with a `sessions` field drop rows below the threshold;
  - `transitions`, which only has `count`, drops rows with a count below the threshold;
  - totals and KPI snapshots stay as they are, since they are rates and counts over the whole site.
- The response carries `suppressed_below: 5`, which the client turns into the "rows from fewer than 5 sessions are hidden" note.
- **Insights are not re-generated.** They come from the full digest and could quote a small number in their evidence. Accepted for now, because insights are aggregate statements by design. If it proves a problem, the analyst can be given the suppressed digest for public websites.

### D3. The dashboard's public mode
- **Entry point:** the server serves `INDEX_HTML` at `/p/{token}` as well as `/`.
  - `app.js` detects `location.pathname` starting with `/p/`.
  - It then skips the session check and calls `/api/public/{token}/…` instead of `/api/sites/{slug}/…`.
- **Navigation:** limited to Insights, KPIs and Behaviour. The behaviour view includes errors.
- **Controls:** with no session, `can()` returns false for every role, so status menus, Run analysis, privacy and setup controls never render.
- **Labelling:** a banner says "Public, read-only view of <website>". The header shows the Trace logo linking to the marketing site, and no website switcher or account menu.
- **Same code path:** no second app. Views read their endpoint through one `apiBase()` helper.

### D4. Setup page card
- "Public view" card for admins, with three states:
  - off: a short explanation and a "Make public" button;
  - on: the link with Copy, plus "Replace link" and "Make private".
- Users see whether the view is on, and the link, but no buttons.

### D5. One shop source, two hosts
- **Base path:** `shop.js` reads `BASE` from `document.documentElement.dataset.base` (default `""`).
  - Every internal link and route is built as `BASE + path`, and `render()` strips `BASE` from `location.pathname` before matching.
  - The collector's `data-routes` becomes `{{BASE}}/products/:id`.
- **Reviews:** `data-reviews="api"` (default, used by `trace serve`) fetches `/api/reviews/{id}`, as today. `data-reviews="static"` fetches `{BASE}/api/reviews/{id}.json`, and 3 times in 10 asks for `{id}-unavailable.json` instead. GitHub Pages answers that with a real 404, so the failed request and the console error are recorded exactly as on the server.
- **Line references:** the simulated error's `/shop.js:315` line reference in `demo.rs` stays in sync. A test reads `shop.js` and asserts that the line it names throws on `pricing.yearly`.
- **Pages workflow** (`pages.yml`):
  1. copy `site/` to `_site/`, and `crates/trace-server/assets/shop/{index.html,shop.js}` to `_site/demo/`;
  2. write `_site/demo/api/reviews/101.json` to `105.json` (`{ "rating": 4.6, "count": 128 }`) and a `404.html` that stores the requested path and loads `demo/index.html`, so the shop routes to it;
  3. substitute `{{BASE}}`, `{{TRACE_URL}}`, `{{WRITE_KEY}}` and `{{PUBLIC_URL}}` from repository variables (`TRACE_URL`, `SHOP_WRITE_KEY`, `SITE_WRITE_KEY`, `SHOP_PUBLIC_URL`, `SITE_PUBLIC_URL`). The base is `/<repo>/demo`, or `/demo` with a custom domain.
  4. If `TRACE_URL` is empty, remove the collector `<script>` tags and leave the placeholder links. That makes deploying before hosting safe.
  5. Optionally, if a `TRACE_TOKEN` secret is set, push `site/trace.json` and the shop's `trace.json` with `curl` to `/api/sites/{slug}/config`.
- The workflow also triggers on changes to `crates/trace-server/assets/shop/**`.

### D6. Instrumenting the marketing site
- **Snippet:** both pages get `<script src="{{TRACE_URL}}/d.js" data-key="{{SITE_WRITE_KEY}}" defer>`, substituted in the workflow as above.
- **Events:** CTAs carry `data-trace-event="start_free"` and demo links `data-trace-event="open_demo"`.
- **`site/trace.json`** declares those events and two KPIs:
  - "Demo reach": the share of sessions with `open_demo`;
  - "Start free clicks": the share of sessions with `start_free`.

## Risks / Trade-offs

- **A public link is shareable forever.** Mitigated by rotation and switching off, by only aggregates leaving the server, and by suppression.
- **Anyone can send events to a public demo website,** including junk. That is equally true of every instrumented site today. Only aggregates are shown, and privacy scrubbing still applies. A flood would skew the demo but expose nothing.
- **Two copies of the shop logic diverge.** Avoided: there's one source, and the workflow copies it.
- **GitHub Pages 404 routing returns HTTP 404 for deep links on the first load.** Browsers render the page anyway. The collector reports the page view, not the status, so nothing is mis-recorded.

## Migration Plan

- The new nullable columns are added by `db::migrate`, so every existing website stays private.
- Deploying the site before the hosted server exists is safe (D5, step 4).
- Rollback: drop the public router. The columns can stay.

## Open Questions

- **Consent.** The collector keeps a random visitor id and session id in `localStorage`. Under the EU ePrivacy rules, storage on a visitor's device can need consent unless it is strictly necessary; some regulators exempt privacy-preserving audience measurement under conditions. Before instrumenting the public marketing site, decide whether to:
  - rely on such an exemption;
  - add a consent notice;
  - or build a storage-free collector mode (session-only ids in memory). That would be a separate change.
- The hosted server address and the two public links, which are needed before the demo goes live.
