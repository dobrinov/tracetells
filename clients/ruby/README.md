# tracetells

Report a Ruby server's errors to [Trace](https://tracetells.com): what failed, where in your code, on which route, since which release. Trace groups and counts them, shows how many visits each one broke in the browser, and tells you whether a fix stopped it. No runtime dependencies; Ruby 3.1+.

```ruby
# Gemfile
gem "tracetells"
```

**Rails:** nothing else. The gem configures itself from the environment, adds its Rack middleware, and subscribes to `Rails.error`, which sees unhandled errors in controllers and jobs (Rails 7+). Routes come from Rails (7.1+): `/orders/:id`, never the URL. Errors Rails answers with a 4xx (`RecordNotFound`, `RoutingError`) aren't reported.

**Rack and Sinatra:**

```ruby
# config.ru
require "tracetells"
Tracetells.configure
use Tracetells::Rack # reports exceptions with their route and re-raises them
```

Report an error you handled with `Tracetells.report(error)`, or `Tracetells.report_message("Billing", "no plan for account")`. During a request, its route goes with it.

## Configuration

| Variable | Default | |
|---|---|---|
| `TRACE_SERVER_KEY` | – | The website's server key. Your coding agent creates it (`/trace:setup server errors`), or an admin in the dashboard. Keep it in your credentials, never in the repository. Without it the gem does nothing. |
| `TRACE_ENDPOINT` | `https://app.tracetells.com` | Your own server, when you self-host Trace |
| `TRACE_ENVIRONMENT` | `RAILS_ENV`, `RACK_ENV`, else `production` | |
| `TRACE_RELEASE` | `SOURCE_VERSION`, `HEROKU_SLUG_COMMIT`, `RENDER_GIT_COMMIT`, `GIT_COMMIT` or `GITHUB_SHA`, if set | The release the code belongs to |

Or in code: `Tracetells.configure(key:, endpoint:, environment:, release:)`.

## What it sends, and what it never does

**Sent:** the error's class, its message (with up to three causes), the backtrace (files relative to your project, lines, methods, and whether each frame is your code), the route template, the HTTP method and status, the environment, the release, the Ruby version, when it happened and how often.

**Never sent:** request or response bodies, params, query strings, headers, cookies, sessions, environment variables, source code, host names or who was signed in. There is no option to send them.

Messages and file paths are scrubbed before they leave: emails, tokens, card numbers, IBANs and phone-like numbers are replaced, and home directories are cut from paths. Trace's server scrubs everything again. Keep personal data out of exception messages anyway.

## How it behaves

- Errors are sent from a background thread, in batches, at most once a second. Requests never wait for it. After a fork (Puma, Unicorn) a new thread starts by itself.
- The first occurrence of an error is sent right away; repeats within the next minute are sent once, as a count.
- A full queue, a network error, or Trace answering 429 or 5xx: errors are dropped, and the client backs off (5 seconds, doubling up to 5 minutes). It never raises into your code.
- At exit it sends what's queued, waiting at most two seconds.

Licence: MIT.
