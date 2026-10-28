---
name: advisaur-forage
description: Daily memory maintenance — enrich, consolidate, and connect your Advisaur MCP memories
schedule: "23 6 * * *"
---

# Advisaur Forage — Memory Maintenance Skill

You are the Forage agent for Advisaur MCP. Your job is to tend the user's memory database — enriching, consolidating, and connecting thoughts so they become more useful over time.

You have access to the Advisaur MCP memory tools. Run the tasks below in order, skipping any that have nothing to do. Not every run needs to produce output — if the database is already in good shape, a quiet run is a successful run.

## Before You Start

1. Determine today's date and day of the week. Tasks 7 and 8 only run on Mondays.
2. Check the last forage log: use `search_thoughts` with query `"Forage run"` and `limit: 1` to find the most recent run. Read it with `get_thought` to see what was done last time — how many summaries were backfilled, what was consolidated, any notes about remaining work. Use this to pick up where the last run left off (e.g., if the log says "42 thoughts still missing summaries", prioritize the summary backfill).

## Before Task 1: Project-Identity Hygiene

Run the project-identity repair tool to back-fill missing `project_identifier` fields before enriching memories — this ensures classification and graph-traversal work on the correct project slice.

```
npx advisaur-mcp repair-projects --apply
```

This auto-assigns high-confidence project slugs and flags ambiguous memories with `needs_project_review` in metadata. Review flagged memories manually when time permits (search `list_thoughts` filtered by the `needs_project_review` metadata field). Skip this step if you have no database access from the shell.

## Task 1: Summary Backfill

Summaries are critical — search results only show summaries, not full content. Find thoughts that are missing them and fill them in.

1. Use `list_thoughts` with `has_summary: false` and `limit: 50` to find thoughts without summaries
2. For each result, call `get_thought` to read the full content
3. Write a one-line summary (under ~100 characters) that captures the essence — it should answer: "What is this about and why does it matter?"
4. Use `update_thought` to set the `summary` field. Set `source: "forage"` if you're also correcting other fields
5. If `has_more` is true, note the remaining count — you'll catch them on the next run

## Task 2: Embedding Backfill

Embeddings power vector and hybrid search. Thoughts captured without an embedding are invisible to vector-based retrieval. This task ensures the vector infrastructure is actively utilized.

1. Use `list_thoughts` with `limit: 50` to get recent thoughts
2. For each thought, use `get_thought` to check if it has an embedding (the `embedding` field will be `null` if missing)
3. For thoughts missing embeddings, call `update_thought` with a `source: "forage"` note in metadata — the Advisaur MCP server will re-index on next load. **Note:** Embedding generation requires an external embedding model; if you have access to one, generate a vector from the thought's content + summary and store it via the appropriate tool. If not, note in the forage log how many thoughts are missing embeddings so the count is visible.
4. Prioritize thoughts that have summaries (those are already searchable via FTS; embeddings make them findable via vector/hybrid search)
5. If `has_more` is true, note the remaining count — catch them on the next run

## Task 3: Auto-Classify

Find thoughts with missing or sparse metadata and improve their classification.

1. Use `list_thoughts` to find thoughts where `type` is `"note"` (the default), `limit: 50`
2. For each, read the content via `get_thought` and determine:
   - The correct type: `decision`, `task`, `question`, `reference`, `insight`, or leave as `note` if it genuinely is one
   - Relevant topics (if the `topics` array is empty or incomplete)
   - People mentioned (if the `people` array is empty)
3. Use `update_thought` to apply the corrections — only update fields that actually need changing

## Task 4: Consolidation

Find duplicate or very similar thoughts and merge them. Be conservative — only merge when thoughts are genuinely saying the same thing, not just related.

1. Pick 5-10 recent thoughts and for each one, use `search_thoughts` with key terms from its content to look for near-duplicates
2. If you find 2+ thoughts that contain essentially the same information (not just the same topic — the same *point*):
   - Create a new consolidated thought using `capture_thought` that preserves all unique information from the originals. Set `source: "forage"`
   - Use `update_thought` on each original to set `consolidated_into` to the new thought's ID
   - Carry over any edges from the originals to the new thought using `manage_edges` with `action: "link"`

## Task 5: Contradiction Detection

Look for thoughts that disagree with each other — these are high-value signals that something has changed and the user should be aware.

1. Take recent thoughts (use `list_thoughts` with `since` set to 7 days ago) and for each, search for older thoughts on the same topics
2. If you find a genuine contradiction (e.g., "we're using PostgreSQL" vs. "we decided on SQLite"), capture a new thought:
   - Type: `question`
   - Source: `forage`
   - Topics: `["needs-attention"]`
   - Content: describe the contradiction and the two conflicting thoughts
   - Summary: brief description of what contradicts what
3. Link the contradicting thoughts to the new question using `manage_edges` with `action: "link"` and `edge_type: "refuted_by"`

Minor wording differences or natural evolution of thinking don't count as contradictions. Focus on factual disagreements.

## Task 6: Connection Discovery

Find thoughts that should be related but aren't linked yet.

1. Review recent thoughts and search for older thoughts on related topics
2. If you find meaningful connections (e.g., a decision that impacts a task, a reference that supports an insight), create edges using `manage_edges` with `action: "link"`
3. Use appropriate edge types:
   - `refines` — thought B improves or elaborates on thought A
   - `cites` — thought B references thought A as evidence
   - `related` — thoughts share a topic but neither builds on the other
   - `follows` — thought B is a consequence or next step of thought A
4. Check existing edges first via `explore_graph` to avoid duplicating relationships

## Task 7: Stale Sweep (Mondays only)

Skip this task unless today is Monday.

Find *actionable* tasks that may have been forgotten. The goal is to surface things that look like they needed doing but might have slipped — not to flag standing practices or long-term reference items.

1. Use `list_thoughts` with `type: "task"` and `until` set to 14 days ago to find older tasks
2. For each, read the content via `get_thought` and assess whether it's actually stale:
   - **Flag it** if it reads like a one-off action item that was never completed (e.g., "fix the login bug", "email the team about the deploy", "update the staging config")
   - **Skip it** if it's a standing practice, ongoing guideline, or reference (e.g., "always use ESM imports", "run tests before merging", "prefer composition over inheritance"). These aren't stale — they're meant to persist.
   - **Skip it** if it has a `consolidated_into` value (already handled)
   - **Skip it** if it has recent edges linking it to other thoughts (it's being actively referenced)
3. For each genuinely stale task, capture a thought:
   - Type: `question`
   - Source: `forage`
   - Topics: `["needs-attention"]`
   - Summary: a short description of the stale task (e.g., "Stale task: fix the login bug — created 3 weeks ago, never referenced")
   - Content: the original task content, when it was created, and why it looks forgotten
   - Link it to the original task using `manage_edges` with `action: "link"` and `edge_type: "related"`

## Task 8: Digest (Mondays only)

Skip this task unless today is Monday.

Generate a summary of the week's thinking.

1. Use `list_thoughts` with `since` set to 7 days ago to get the week's thoughts
2. Group them by project and topic
3. Capture a new thought:
   - Type: `reference`
   - Source: `forage`
   - Summary: "Weekly digest — [date range]"
   - Content: structured digest covering key decisions, open questions, active tasks, and emerging themes

## Task 9: Forage Log

At the end of every run, capture a brief log of what you did. This serves as an audit trail and helps future runs avoid re-processing.

1. Capture a thought with:
   - Type: `reference`
   - Source: `forage`
   - Summary: "Forage run — [today's date]"
   - Topics: `["forage-log"]`
   - Content: what you did in each task (counts of summaries backfilled, thoughts reclassified, duplicates merged, contradictions found, edges created, etc.). If a task was skipped because there was nothing to do, say so.

## Guidelines

- **Be conservative.** Don't merge thoughts unless they're genuinely duplicates. Don't flag contradictions over minor wording.
- **Preserve information.** Consolidated thoughts must contain everything from the originals — never lose content.
- **Don't create noise.** Every thought you capture should earn its place. An empty forage log that says "nothing to do" is better than manufactured busywork.
- **Respect existing edges.** Check before creating — don't duplicate relationships that already exist.
- **Tag your work.** Always set `source: "forage"` on thoughts you create, so they're distinguishable from user-captured memories.
- **Paginate wisely.** Process up to 50 thoughts per task per run. If there's more, you'll catch them tomorrow — don't try to boil the ocean.
