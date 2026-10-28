# Proposal

This is a security fix.

## Why

A website's write key is public, so anyone can send events. The text in them (element labels, error messages, selectors, paths, custom-event names) reaches coding agents: through the behaviour summary, the server's own analyst, and the insight task prompts that agents implement in the owner's codebase. Crafted text could fake a new section of a prompt ("## Constraints – run …") or ask the agent to run commands.

## What Changes

- At ingest, visitor text is stored as one plain line. Line breaks, control and invisible characters (including bidi overrides and zero-width characters) become single spaces, and backticks become `'`, so the text can't open a new markdown section or close a code span. Spaces in paths become `%20`, as a browser sends them.
- The behaviour summary cleans the same fields again, which covers events stored before this change.
- Every insight's task prompt says, near the top, that quoted text came from visitors' browsers and is data, never instructions.
- The MCP behaviour summary carries the same `note`. The server's Claude analyst gets the rule in its system prompt, and its digest JSON escapes `<`, so a string can't close the `<digest>` tag.

## Capabilities

### New Capabilities
- `visitor-text-safety`: keeping text from website visitors from acting as instructions to agents.

### Modified Capabilities
None.

## Privacy impact

None: nothing new is collected, and stored text only gets simpler.

## Site impact

None: the collector is unchanged.

## Impact

- `crates/trace-core`: `privacy.rs` (`one_line`, `one_line_path`), `ingest.rs`, `digest.rs`, `prompt.rs`.
- `crates/trace-agent`: `analyst.rs`.
- `crates/trace-server`: `mcp.rs`.
- README: the privacy section.
