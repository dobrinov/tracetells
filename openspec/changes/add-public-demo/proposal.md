# Proposal

This is a feature.

## Why

People deciding whether to trust Trace want to see it working before they sign up: what it records, what it refuses to record, and the insights it produces. Today the Stride demo shop only runs locally with `trace serve --demo-site`, and the dashboard needs an account. A public, live demo makes the privacy promise checkable: anyone can click around the shop, then look at exactly what Trace kept.

## What Changes

- **Public read-only view per website.** An owner or admin can switch on a public link for one website. Anyone with the link sees that website's KPIs, behaviour, errors, insights (with their prompts) and evaluations, without signing in. They never see settings, people, roles, API tokens, the write key, privacy rules, the activity log, plan or usage, and nothing in the public view can change data. The link can be turned off or replaced with a new one at any time.
- **Small-number suppression in the public view.** Any aggregate built from fewer than 5 sessions is left out of the public view, so a quiet website can't expose one visitor's path.
- **The marketing site is instrumented** with the hosted Trace server, with its own `trace.json` (KPIs such as visits that reach the demo or click "Start free").
- **The Stride shop moves into the marketing site** at `site/demo/`, served by GitHub Pages and instrumented as its own website. The shop's source stays in one place (`crates/trace-server/assets/shop/`); the Pages workflow copies it in and fills in the base path, the Trace server address and the write key. `trace serve --demo-site` keeps working.
  - Deep links (`/demo/products/shoe-1`) work through a `404.html` that hands the path to the shop.
  - The flaky reviews API becomes static JSON files; in the static build the shop asks for a missing file about 30% of the time, which is still a real failed request that Trace records.
- **The marketing site links to the live demo:** "Try the demo shop", then "See what Trace recorded" (the shop's public view) and "See this page's own analytics" (the marketing site's public view).
- The Trace server address, the two write keys and the two public links are placeholders (repository variables) until the hosted domain exists.

## Capabilities

### New Capabilities
- `public-view`: a per-website, revocable, read-only public link to a website's aggregated analytics, what it shows and hides, and small-number suppression.
- `live-demo`: the public demo on the marketing site, meaning the static Stride shop with its planted problems, the instrumented marketing site, and the links between them and their public views.

### Modified Capabilities
None. The existing changes (`add-accounts-plans-and-sites`, `add-onboarding`) are not archived yet, so there are no main specs to modify.

## Privacy impact

- **Collected:** the marketing site and the demo shop send the same events as any instrumented site, scrubbed in the browser by `d.js` and again on the server by `privacy.rs`. Nothing new is collected. Like every Trace site, the collector keeps a random visitor id and session id in the visitor's `localStorage`; see the open question about consent in design.md.
- **Exposed:** this is the first time aggregated analytics are visible without signing in, and only for websites whose owner switches it on. The public view shows only aggregates, never single sessions, and suppresses any aggregate from fewer than 5 sessions. It never shows the write key, privacy rules, people or tokens. The public link token is random, and switching it off or regenerating it makes the old link stop working immediately.
- **Stored:** one nullable public token, plus when and by whom it was switched on, per website. Switching the view on or off is written to the account's activity log.

## Site impact

None for the collector: `d.js` is unchanged. The marketing site and the demo shop load the existing collector with `defer`, like any instrumented site. The static shop adds no dependencies.

## Impact

- `crates/trace-core`: `projects` gains `public_token`, `public_since` and `public_by`; a public digest with small-number suppression.
- `crates/trace-server`:
  - new unauthenticated `GET /api/public/{token}/…` routes (overview, KPIs, digest, insights, one insight and its prompt, runs);
  - admin routes to switch the view on, off or regenerate it;
  - the dashboard gains a public, read-only mode and a "Public view" card on the Setup page;
  - `shop.js` gains a base path and a static reviews mode.
- `site/`: collector snippets and `data-trace-event` attributes on the CTAs, `site/trace.json`, demo links, and `404.html` routing for `site/demo/`.
- `.github/workflows/pages.yml`: copies the shop into the artifact and fills in placeholders from repository variables; optionally pushes both `trace.json` files with a `TRACE_TOKEN` secret.
- README: the public view and the live demo.
