# @tracetells/node

Report a Node.js server's errors to [Trace](https://tracetells.com): what failed, where in your code, on which route, since which release. Trace groups and counts them, shows how many visits each one broke in the browser, and tells you whether a fix stopped it. No runtime dependencies; Node 18+ (and Bun). TypeScript types included.

```sh
npm install @tracetells/node
```

```js
import * as trace from "@tracetells/node";

// Reads TRACE_SERVER_KEY, TRACE_ENDPOINT, TRACE_ENVIRONMENT and TRACE_RELEASE.
// Without a key it does nothing. Uncaught exceptions and unhandled rejections are reported.
trace.init();
```

**Express**, after your routes and before your own error handler:

```js
app.use(trace.expressErrorHandler()); // reports 5xx errors with their route, then passes them on
```

**Fastify:**

```js
await app.register(trace.fastifyPlugin);
```

**Plain `http`:**

```js
http.createServer(trace.wrapHandler(async (req, res) => { /* … */ }, (req) => "/orders/:id"));
```

Report an error you handled with `trace.report(err, { route: "/orders/:id", method: "GET", status: 500 })`, or `trace.reportMessage("Billing", "no plan for account")`. Call `await trace.close()` on shutdown to send what's queued.

## Configuration

| Variable | Default | |
|---|---|---|
| `TRACE_SERVER_KEY` | – | The website's server key. Your coding agent creates it (`/trace:setup server errors`), or an admin in the dashboard. Keep it in your secret store, never in the repository. |
| `TRACE_ENDPOINT` | `https://app.tracetells.com` | Your own server, when you self-host Trace |
| `TRACE_ENVIRONMENT` | `NODE_ENV`, else `production` | |
| `TRACE_RELEASE` | `SOURCE_VERSION`, `RENDER_GIT_COMMIT`, `VERCEL_GIT_COMMIT_SHA`, `GIT_COMMIT` or `GITHUB_SHA`, if set | The release the code belongs to |

Or in code: `trace.init({ key, endpoint, environment, release })`.

## What it sends, and what it never does

**Sent:** the error's class, its message (with up to three causes), the stack (files relative to your project, lines, functions, and whether each frame is your code), the route template (`/orders/:id`, never the URL), the HTTP method and status, the environment, the release, the runtime, when it happened and how often.

**Never sent:** request or response bodies, parameters, query strings, headers, cookies, environment variables, source code, host names or who was signed in. There is no option to send them.

Messages and file paths are scrubbed before they leave: emails, tokens, card numbers, IBANs and phone-like numbers are replaced, and home directories are cut from paths. Trace's server scrubs everything again. Keep personal data out of error messages anyway.

## How it behaves

- Errors are sent in the background, in batches, at most once a second. Requests never wait for it.
- The first occurrence of an error is sent right away; repeats within the next minute are sent once, as a count.
- A full queue, a network error, or Trace answering 429 or 5xx: errors are dropped, and the client backs off (5 seconds, doubling up to 5 minutes). It never throws into your code.
- An uncaught exception is sent (waiting at most two seconds), then the process prints it and exits with code 1, as Node does without the client. If your app has its own `uncaughtException` handler, the client leaves that decision to it.

Licence: MIT.
