# Trace for Claude Code

[Trace](https://tracetells.com) measures how people use a website, in aggregate and without storing anything on their devices: rage and dead clicks, errors, forms people give up on, and your KPIs. This plugin connects Claude Code to it and teaches Claude the three jobs:

| Skill | What it does |
|---|---|
| `/trace:setup` | Adds the website, installs the snippet, writes `trace.json` with KPIs and events, and masks screens that show customer data |
| `/trace:analyse` | Reads the website's behaviour, finds the few changes most likely to move its KPIs, and saves the ones you approve as insights |
| `/trace:fix` | Implements an insight in your code and marks it released, so Trace measures whether the KPI moved |

You can also just ask: "set up Trace on this site", "what should I fix?", "work on the next Trace insight".

## Install

```
/plugin marketplace add dobrinov/tracetells
/plugin install trace@tracetells
```

Then run `/mcp`, choose **plugin:trace:trace** and **Authenticate**. Your browser opens Trace to sign in and approve it; there's no token to paste. Don't have an account? [Start free](https://app.tracetells.com/#/signup).

If you added Trace by hand before (`claude mcp add ... trace`), remove that one with `claude mcp remove trace`, so Claude doesn't see two.

## Self-hosted Trace

Point the plugin at your own server when it asks for **Trace MCP server**, or later with `/plugin configure trace@tracetells`, e.g. `https://trace.example.com/mcp`.

## Privacy

Claude only ever sees aggregates: counts and rates across visits, never one visitor. Trace sends no data to an AI provider itself; the analysis runs in your Claude Code session. See [how Trace handles data](https://tracetells.com/privacy.html).
