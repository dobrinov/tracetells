# tracetells

Report a Rust server's errors to [Trace](https://tracetells.com): what failed, where in your code, on which route, since which release. Trace groups and counts them, shows how many visits each one broke in the browser, and tells you whether a fix stopped it.

```toml
[dependencies]
tracetells = { version = "0.1", features = ["axum", "tracing"] }
```

```rust
#[tokio::main]
async fn main() {
    // Reads TRACE_SERVER_KEY, TRACE_ENDPOINT, TRACE_ENVIRONMENT and TRACE_RELEASE.
    // Without a key it does nothing. Keep the guard: dropping it flushes at shutdown.
    let _trace = tracetells::init(tracetells::Config::from_env());

    let app = axum::Router::new()
        .route("/orders/{id}", axum::routing::get(show))
        .layer(tracetells::TraceLayer); // 5xx responses and panics, with their route

    // With the `tracing` feature: events logged at error level are reported too.
    // tracing_subscriber::registry().with(tracetells::TracingLayer).init();
}
```

Report an error you handled with `tracetells::report(&err)`, or `tracetells::report_message("Billing", "no plan for account")`.

## Configuration

| Variable | Default | |
|---|---|---|
| `TRACE_SERVER_KEY` | – | The website's server key. Your coding agent creates it (`/trace:setup server errors`), or an admin in the dashboard. Keep it in your secret store, never in the repository. |
| `TRACE_ENDPOINT` | `https://app.tracetells.com` | Your own server, when you self-host Trace |
| `TRACE_ENVIRONMENT` | `production` in release builds, `development` otherwise | |
| `TRACE_RELEASE` | `SOURCE_VERSION`, `RENDER_GIT_COMMIT`, `VERCEL_GIT_COMMIT_SHA`, `GIT_COMMIT` or `GITHUB_SHA`, if set | The release the code belongs to, so Trace can say "new in v2.4" or "back in v2.6" |

The same settings can be given in code: `Config::default().key(...).endpoint(...).environment(...).release(...)`.

## What it sends, and what it never does

**Sent:** the error's type, its message, the backtrace (files relative to your project, lines, functions, and whether each frame is your code), the route template (`/orders/:id`, never the URL), the HTTP method and status, the environment, the release, the Rust version, when it happened and how often.

**Never sent:** request or response bodies, parameters, query strings, headers, cookies, environment variables, local variables, source code, host names or who was signed in. There is no option to send them.

Messages and file paths are scrubbed before they leave: emails, tokens, card numbers, IBANs and phone-like numbers are replaced, and home directories are cut from paths. Trace's server scrubs everything again. Keep personal data out of error messages anyway: a name in a message is something no pattern can catch.

## How it behaves

- Errors leave from a background thread, in batches, at most once a second. Your requests never wait for it.
- The first occurrence of an error is sent right away; repeats within the next minute are sent once, as a count.
- A full queue, a network error, or Trace answering 429 or 5xx: errors are dropped, and the client backs off (5 seconds, doubling up to 5 minutes). It never panics or returns an error to your code.
- A panic is sent before the panic continues as it would without the client, waiting at most two seconds.

Licence: MIT.
