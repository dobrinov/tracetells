---
name: setup
description: Set up Trace analytics on the website in this codebase - add the website, install the collector snippet, write trace.json with KPIs and custom events, and mask screens that show customer data. Use when the user says "set up Trace", "add Trace to this site", "install the Trace snippet", "define KPIs for Trace" or "track events with Trace".
---

# Set up Trace on this website

Trace measures how people use a website, in aggregate, and turns it into improvement insights tied to the team's KPIs. Use the `trace` MCP tools for every step. Show the person each change to their code before you make it.

1. **The website.** Call `list_sites`. If none of them is this product, call `add_site` with its domain as the name. When the account has several websites, pass `site` to every other tool.
2. **The snippet.** Call `get_install_snippet` and add the tag to the HTML every page shares (layout, template or index.html), before `</head>`, once. For a single-page app, add `data-routes` for dynamic URLs as the tool explains. It is cookieless by default: it stores nothing on visitors' devices and needs no consent banner. Only if the site already has a consent tool, pass `consent: "required"` and have that tool call `trace.consent(true)`.
3. **Custom events.** Find the moments that matter in this product (sign-up, checkout, a key action) and send them with `trace.track("name", { prop })` from code, or `data-trace-event="name"` on an element. Props must be low-cardinality values, never ids, names or emails.
4. **KPIs as code.** Write `trace.json` at the repository root: the custom events with one-line descriptions, and 3 to 8 KPIs for what this product is for, each a `session_rate` or a `conversion` with a goal and a target. Validate with `check_config`, then apply with `push_config`. Keep the file in the repository as the source of truth.
5. **Customer data.** Find screens that show names, emails, account details or orders, and put `data-trace-mask` on their container: no text is read inside it. Use `add_privacy_rule` with `ignore_path` for admin areas.
6. **Signed-in users (optional).** `trace.identify(id)` with an opaque internal id makes a signed-in person count once across days. Call it on every page. Leave it out where it doesn't fit, such as a site for children, and mention it in the site's privacy policy if used.
7. **Check.** After the person deploys, call `get_setup_status` until events arrive.

Never put a write key, email or personal value into event names or props.
