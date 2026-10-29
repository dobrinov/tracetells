# Proposal

This is a change to an existing feature.

## Why

Analysis runs in the owner's coding agent, so the dashboard has to tell them when to run it. It said so in one grey line on Insights. The owner couldn't see how old the analysed behaviour was, how much had arrived since, or that they should run `/trace:analyse` now.

## What Changes

- **Analysis status** gains `next` (analyse, wait, nothing_new), `new_events`, `needed`, `analysed_by` (the agent's app, from its token's name, or "Trace") and `agent_connected`. Its message says how long ago the last analysis was. `projects.analysed_by` stores who ran it.
- **Analysis panel** on Home and Insights: last analysis and who ran it, the time the analysed behaviour ends, new sessions and events since, progress to the threshold, and what to do: the command with a Copy button when an agent is connected, otherwise connect one or analyse on the server. It is prominent when it's worth analysing, and quiet below the list otherwise.
- **Header button** reads "Run analysis · N new" and stands out when it's worth analysing.
- While the panel asks for an analysis, it checks every 20 seconds and refreshes the page when the agent saves one.

## Capabilities

### Modified Capabilities
- `analysis-readiness`: the status says what to do and how old the last analysis is, and the dashboard shows it on Home and Insights.

## Privacy impact

None: counts only. `analysed_by` is an app name such as "Claude Code", never a person.

## Site impact

None.
