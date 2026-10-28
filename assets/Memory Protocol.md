# Advisaur MCP — Memory Protocol

You have persistent memory via Advisaur MCP tools. Memory survives across sessions and is shared across all AI tools the user works with. You MUST use it — do not rely on conversation context alone.

## When to SAVE (mandatory)

Call `capture_thought` after any of these events:

- **Decisions**: Architecture choices, library selections, tradeoffs
- **Preferences**: User likes/dislikes, workflow habits, coding style
- **People & roles**: Who does what
- **Project context**: Goals, deadlines, constraints, scope changes
- **Bugs & fixes**: Root cause discoveries, workarounds
- **Architecture & patterns**: System design, data flow, conventions
- **Insights**: Non-obvious learnings, things that surprised you

Always include: a `summary` (one-line, <100 chars), a `type`, relevant `topics`, and link to `related_to` thoughts when applicable.

## When to SEARCH (mandatory)

Call `search_thoughts` or `list_thoughts` before:

- Starting work on any task
- Making a decision — check for prior decisions on the same topic
- When something feels familiar — it probably is
- After context compaction — immediately search to recover session context
- When the user says "remember", "recall", "what do we know about", "what did we decide"

## What NOT to save

- Ephemeral debugging output (stack traces, log lines)
- Code content already in git — save the *decision*, not the code
- Transient conversation — save the conclusion, not the process
- Duplicates — search first, use `update_thought` instead of creating new ones
