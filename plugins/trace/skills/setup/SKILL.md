---
name: setup
description: Set up Trace analytics on the website in this codebase - add the website, install the collector snippet, write trace.json with KPIs and custom events, mask screens that show customer data, and report server errors with the tracetells client for Rust, Node.js or Ruby. Use when the user says "set up Trace", "add Trace to this site", "install the Trace snippet", "define KPIs for Trace", "track events with Trace" or "set up server errors".
---

# Set up Trace on this website

Trace measures how people use a website, in aggregate, and turns it into improvement insights tied to the team's KPIs. Use the `trace` MCP tools for every step. Show the person each change to their code before you make it.

1. **The website.** If trace.json already names one under `"site"`, use it. Otherwise call `get_connection` to see the account and its websites, ask the person which one this product is, or call `add_site` with its domain as the name if none is. Pass its identifier as `site` to every other tool. If the person expected another account, they need to reconnect (`/mcp`, choose trace, Authenticate) and pick it.
2. **The snippet.** Call `get_install_snippet` and add the tag to the HTML every page shares (layout, template or index.html), before `</head>`, once. For a single-page app, add `data-routes` for dynamic URLs as the tool explains. It is cookieless by default: it stores nothing on visitors' devices and needs no consent banner. Only if the site already has a consent tool, pass `consent: "required"` and have that tool call `trace.consent(true)`.
3. **Custom events.** Find the moments that matter in this product (sign-up, checkout, a key action) and send them with `trace.track("name", { prop })` from code, or `data-trace-event="name"` on an element. Props must be low-cardinality values, never ids, names or emails.
4. **KPIs as code.** Write `trace.json` at the repository root, starting with `"site"`: the website's identifier, so every later session and `trace push` know which website this repository is (and `"server"` when it isn't the hosted app.tracetells.com). Then the custom events with one-line descriptions, and 3 to 8 KPIs for what this product is for, each a `session_rate` or a `conversion` with a goal and a target. Validate with `check_config`, then apply with `push_config`. Keep the file in the repository as the source of truth.
5. **Customer data.** Find screens that show names, emails, account details or orders, and put `data-trace-mask` on their container: no text is read inside it. Use `add_privacy_rule` with `ignore_path` for admin areas.
6. **Signed-in users (optional).** `trace.identify(id)` with an opaque internal id makes a signed-in person count once across days. Call it on every page. Leave it out where it doesn't fit, such as a site for children, and mention it in the site's privacy policy if used.
7. **Check.** After the person deploys, call `get_setup_status` until events arrive.
8. **Server errors.** When the person asks for them (`/trace:setup server errors` goes straight here), or once the rest is done, offer them. Install the client for the server's stack, with the integration for its framework:
   - **Rust:** `tracetells` (features `axum` and `tracing` when used): `let _trace = tracetells::init(tracetells::Config::from_env());` at the start of `main`, `.layer(tracetells::TraceLayer)` on the router, and `TracingLayer` on the subscriber if it uses `tracing`.
   - **Node.js:** `@tracetells/node`: `trace.init()` where the server starts, then `app.use(trace.expressErrorHandler())` after the routes (Express), `await app.register(trace.fastifyPlugin)` (Fastify) or `trace.wrapHandler(...)` (plain `http`).
   - **Ruby:** the `tracetells` gem. Rails needs nothing else; Rack and Sinatra apps add `Tracetells.configure` and `use Tracetells::Rack`.

   Then call `server_key` with `create` (admins) and tell the person to set `TRACE_SERVER_KEY` to the secret in the server's environment or secret store, with `TRACE_ENDPOINT` when the server isn't app.tracetells.com. Never write the key into the repository, a config file or a commit. Set `TRACE_RELEASE` to the commit or version in the deploy if the platform doesn't (the README of each client lists the variables it reads). After the deploy, `list_server_errors` shows errors as they happen; the setup step ticks off with the first one.

Never put a write key, a server key, an email or a personal value into event names, props or the repository.
