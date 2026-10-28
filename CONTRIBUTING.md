# Contributing to Trace

Thanks for helping. Contributions to the community edition come in as **OpenSpec changes**, filed as GitHub issues: you explore the problem and write the proposal, and maintainers review it and build it. That keeps every change thought through before code is written, especially for privacy and for the effect on visitors' browsers, which matter more here than anything else.

## What a contribution looks like

An issue (a feature request, a bug report or another change) holding a complete OpenSpec change, made with [OpenSpec](https://github.com/Fission-AI/OpenSpec) in two steps:

1. **`/opsx:explore`**: map the problem and understand the code involved.
2. **`/opsx:propose`**: draft the change's artifacts:
   - `proposal.md`: why, what changes, the **privacy impact** and the **site impact**
   - `specs/<capability>/spec.md`: the behaviour, as requirements with WHEN/THEN scenarios
   - `design.md`: how, starting from what exploring the code found (optional for small changes)
   - `tasks.md`: the implementation steps, including tests and docs

Bugs work the same way: the proposal describes what happens, what should happen and how to reproduce it, and the spec adds a scenario that fails today.

## Steps

```sh
# 1. Tools: the OpenSpec CLI, and an AI coding agent (Claude Code works out of the box)
npm install -g @fission-ai/openspec

# 2. A fresh checkout of main
git clone https://github.com/dobrinov/tracetells && cd tracetells
openspec init --tools <your-agent>   # only if you don't use Claude Code; adds the /opsx commands for your tool

# 3. In your agent
/opsx:explore   <the problem or idea>
/opsx:propose   <what you want to change>

# 4. Check it, then file it (save your explore findings to a file to include them)
openspec validate <change-id> --strict
scripts/openspec-issue.sh <change-id> feature --exploration notes.md   # or: bug, change; add --dry-run to preview
```

`scripts/openspec-issue.sh` needs the [GitHub CLI](https://cli.github.com). You can also use the issue forms on GitHub and paste each file into its field. In the **Spec deltas** field, start each file with `<!-- file: specs/<capability>/spec.md -->`.

## What happens next

A check runs on every new or edited issue. It rebuilds your change from the issue, runs `openspec validate --strict` and labels the issue **openspec-valid** or **openspec-needs-work**, with the validator's messages in a comment. Edit the issue to fix problems and it checks again.

Maintainers then review valid proposals against [`openspec/config.yaml`](openspec/config.yaml). The questions they ask:

- **Privacy first.** Does it collect, store or expose anything new? Could anything identify a person? Trace studies patterns, not people: no session replays, nothing typed, no per-user views.
- **No cost to visitors.** Does the collector get bigger or do more work? Handlers stay passive and real work happens when the browser is idle.
- **Fits the model.** KPIs and events live in `trace.json`; the community edition stays one binary on SQLite.

Accepted changes are applied with `/opsx:apply` and archived into `openspec/specs/` when they ship. If you'd like to build your own accepted proposal, say so on the issue.

## Not ready for a proposal?

Start a discussion instead. Security problems should never go in a public issue: see [SECURITY.md](SECURITY.md).
