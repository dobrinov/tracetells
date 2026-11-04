# Tasks

## 1. MCP

- [x] 1.1 `add_insights`: validation (required fields, lengths, KPI by key or id, confidence), all-or-nothing, the shared de-duplication, `source: agent`, an `agent:` fingerprint prefix, activity log entry, refused for read-only tokens
- [x] 1.2 `analyse_website` prompt (`prompts/list`, `prompts/get`, the prompts capability) and server instructions that mention it and the untrusted-text rule
- [x] 1.3 `list_insights` returns dismissal reasons
- [x] 1.4 Tests: prompt listed and served, an invalid batch saves nothing, saving, de-duplication by fingerprint, read-only refused; plus an end-to-end run over HTTP

## 2. Copy

- [x] 2.1 Dashboard: Setup and Get started present the agent as the analyst and how to run the prompt; the empty Insights page points to it
- [x] 2.2 README, the marketing site's how-it-works section, hero, MCP card, share description, and the pricing page
