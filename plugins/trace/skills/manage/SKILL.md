---
name: manage
description: Do the day-to-day Trace work the dashboard only shows - pick, dismiss, reopen or release an insight, add or remove a privacy rule, confirm the privacy review, publish or withdraw a website's public link, add a website, apply trace.json, or check whether events arrive. Use when the user says things like "release insight 12", "dismiss insight 4 because...", "mask .account-summary", "make the dashboard public", "add a website called docs", "push trace.json" or "is Trace receiving data".
---

# Manage Trace

## Which website

Read `trace.json` at the repository root: its `"site"` is this repository's Trace website, so pass it as `site` to every tool. If the request names one ("… on wunderkind-bg"), that wins. If neither does, call `list_sites`: with one website, use it; with several, ask the person which one this codebase is, then add `"site"` to trace.json so the next session knows. If the website isn't in the list, call `get_connection`: this connection is for another account, and the person needs to reconnect (`/mcp`, choose trace, Authenticate) and pick the right one.

The Trace dashboard is for looking; changes happen here, through the `trace` MCP tools. Say what you changed when you're done.

Do what the person asks, and nothing they didn't: never dismiss, release, publish, withdraw or remove on your own judgement.

## Insights

- **Pick one to do next:** `update_insight_status` with `selected`.
- **Dismiss one:** `update_insight_status` with `dismissed` and `reason`, in the person's words. Ask for the reason if they didn't give one: the analysis reads it so it stops suggesting the same kind of thing.
- **Release one:** only once the change is live, `update_insight_status` with `released`, and `released_at` set to the deploy's time if it went live earlier than now. Trace then compares its goal over the 14 days before and after.
- **Reopen or restore:** `update_insight_status` back to `in_progress` or `new`.
- **Measure again now:** `refresh_insight`. Trace also does this daily.
- To implement one, use `/trace:fix` instead.

## Privacy

- **See what's protected:** `get_privacy_rules`.
- **Keep more out:** `add_privacy_rule` with `mask_selector` (no text read inside an element, e.g. `.account-summary`), `redact_pattern` (a regular expression replaced everywhere, with `replacement`), `ignore_path` (nothing recorded on matching pages, e.g. `/admin*`) or `drop_prop` (a custom-event property never stored). When you can edit the code, prefer `data-trace-mask` on the element.
- **Remove a rule:** `remove_privacy_rule` with its id, only when asked.
- **Confirm the privacy review:** `confirm_privacy_review`, only when the person says they've checked what Trace collects.

## Sharing

- **Public link:** `public_view` with `status`, `publish` (admins), `replace` (a new link; the old one stops working) or `withdraw`. Only publish when asked: anyone with the link sees the website's aggregated analytics.

## Websites and setup

- **Add a website:** `add_site` with its name, then add the snippet it returns (see `/trace:setup`).
- **Apply trace.json:** `check_config`, then `push_config` with the file's contents. In CI, `trace push` does the same.
- **Is data arriving?** `get_setup_status`, and `get_tracking_health` for each declared event.
