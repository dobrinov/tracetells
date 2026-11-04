# Proposal

This is a change to trace.json and the MCP server.

## Why

With the work done in the agent, nothing told it which website a repository is: it guessed from list_sites in every session, and commands copied from the dashboard (`/trace:fix 12`) didn't say which website they meant. Which account a connection works in was only visible on the approval page.

## What Changes

- **`"site"` and `"server"` in trace.json:** the website's identifier and, when self-hosted, the server. Validated; `trace check` prints them; `trace push` uses them when `--site` and `--server` aren't given; applying the file to a different website (API, MCP or CLI) is refused.
- **`get_connection` MCP tool:** the server, the account, who the connection acts as, its role, whether it's read-only, and the account's websites.
- **Agents read trace.json first:** the MCP instructions, the analyse prompt and every plugin command use its `"site"`, fall back to asking and writing it, and point to reconnecting when the website is in another account. The error for an unknown website says so too.
- **Dashboard commands name the website** in accounts with several (`/trace:fix 12 on wunderkind-bg`).

## Capabilities

### Modified Capabilities
- `config`: trace.json names its website and server.

## Privacy impact

None.

## Site impact

None.
