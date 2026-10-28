# Changelog

All notable changes to Advisaur MCP (called ShelbyMCP before ADR-0012) will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **Renamed to Advisaur MCP** (ADR-0012). The npm package and binary are `advisaur-mcp` (platform packages `advisaur-mcp-<platform>`), the MCP Registry entry is `io.github.Studio-Moser/advisaur-mcp`, the config key clients get is `advisaur-mcp`, the skills are `advisaur-forage` and `advisaur-onboard`, and the Claude Code plugin is `advisaur`. `serverInfo` is `advisaur-mcp` ("Advisaur memory") and the status resource `advisaur-mcp://status`. The crates are `assistant-memory`, `assistant-integrations`, `assistant-mcp` and `assistant-memory-eval`.
- **Coming from ShelbyMCP:** `~/.shelbymcp` moves to `~/.studiomoser-assistant-mcp` the first time the server runs with its default database. `SHELBY_DB_PATH`, `SHELBY_TRANSPORT` and `SHELBY_API_KEY` are now `ADVISAUR_*`; the old names are read, with a warning, for one release. `setup` replaces a client's `shelbymcp` entry, keeping a command you wrote yourself, and `uninstall` removes either. The last `shelbymcp` release runs `advisaur-mcp` and says so. The brief header is `## Advisaur memory context`.

### Added

- Rust crates for the SQLite memory engine, reusable client integrations, and the stdio/HTTP MCP server.
- OAuth 2.1 for hosted Streamable HTTP deployments, with bearer-key authentication retained.
- Native npm packages for macOS ARM64/x64, Linux ARM64/x64, and Windows x64, plus current client plugin packages.
- A protected, approval-gated release workflow that builds and verifies every artifact without publishing by default.
- Schema migrations v9-v11, aligned with Shelby-MacOS: local search telemetry, the local feedback log, and thought re-confirmation timestamps.
- `capture_thought` responses now include an `action` describing whether the thought was created, reinforced as a duplicate, or returned with relationship suggestions.
- `search_thoughts` now accepts a canonicalized `topic` filter.
- Canonical immutable project UUIDs, alias-aware project resolution, and additive `project_id` inputs and outputs for project-scoped MCP tools.
- Curated `get_brief` policy with trusted exact-project/shared eligibility, sensitivity and prompt-injection filtering, consolidation/refutation handling, deterministic role lanes, summary deduplication, an 800-token default budget, structured items, and omission diagnostics. The MCP schema now exposes `include_shared`; `select_context` reuses the same curated essentials policy.
- Claim-scoped refutation. A `refuted_by` edge whose `metadata` carries a non-empty string `claim` now supersedes **only that claim**: the source thought stays brief-eligible and renders as `- <summary> (superseded: <claim>)`. `get_brief` items gain a `refuted_claims` array. Previously refutation was all-or-nothing, so correcting one claim in a multi-claim thought dropped every still-valid claim it carried out of every brief. See [ADR 0001 §5a](https://github.com/Studio-Moser/Shelby-Docs/blob/main/docs/adr/0001-memory-server-architecture-contract.md).

### Changed

- `npx shelbymcp` now launches the platform-native Rust binary; the server performs no inference or automatic embedding generation.
- Client setup prefers current plugin/extension packages and keeps safe CLI configuration as a fallback without editing global instruction files.
- `search_thoughts`, `list_thoughts`, `get_thought`, `select_context`, `explore_graph`, `expand_neighbors`, and `capture_thought` relationship suggestions now wrap non-trusted thought bodies and summaries in an explicit data-only quarantine fence. `select_context` also keeps non-trusted topics and people inside that fence, while `get_thought` warns that every structured field is untrusted data. Unknown trust levels fail closed as unverified; trusted text remains unchanged.
- `get_thought` now reinforces the retrieved thought and returns its updated reinforcement count.
- The legacy `preference` thought type is normalized to `decision` on both capture and update.
- **Breaking (output shape):** `get_brief` items now always include a `refuted_claims` array, and a thought carrying a scoped refutation renders with a `(superseded: …)` caveat appended to its line. Consumers that parse brief markdown or item objects should tolerate both. `policy_version` bumped 1 → 2 to signal the change; the canonical cross-codebase fixture is at `fixture_version` 2.
- A `refuted_by` edge with no `claim`, a blank `claim`, or a non-string `claim` continues to refute the whole thought. Existing edges carry no `metadata`, so stored data and current behaviour are unaffected.
- Claim text passes the same normalization, prompt-injection and PII gate as summaries before reaching a brief; a claim that fails the gate is dropped without affecting its thought's eligibility.

- Project-scoped reads and updates now use UUID identity while retaining current slugs as compatibility fields.
- Schema version stamp bumped from 4 → 5 to align with Shelby-MacOS's Swift memory implementation, per [ADR 0001 §8](https://github.com/Studio-Moser/Shelby-Docs/blob/main/docs/adr/0001-memory-server-architecture-contract.md). No actual schema change — the v5 migration is a no-op version stamp. Both codebases now advance through the same migration sequence so cross-codebase tooling and conformance tests can rely on a single number.
- Logged server version constant updated from `0.1.0` → `0.3.0` to match `package.json`. Comment added to keep them in sync.

## [0.1.0] - 2026-03-28

### Added

- MCP server with stdio JSON-RPC protocol via `@modelcontextprotocol/sdk`
- SQLite database with WAL mode, FTS5 full-text search, and vector similarity
- Knowledge graph with typed edges (refines, cites, refuted_by, tags, related, follows)
- 9 focused MCP tools: capture_thought, search_thoughts, list_thoughts, get_thought, update_thought, delete_thought, manage_edges, explore_graph, thought_stats
- Forage scheduled skill with 8 tasks (summary backfill, auto-classify, consolidation, contradiction detection, connection discovery, stale sweep, digest, forage log)
- CLI commands: `setup <agent>`, `uninstall <agent>`, `protocol`, `forage`, `help`
- Auto-setup for 6 agents: Claude Code CLI, Claude Desktop, Cursor, Codex, Windsurf, Gemini CLI
- `--forage` flag on setup to install the Forage skill alongside the MCP server
- Memory Protocol — copy-paste instructions that tell agents when to save/search
- Agent setup docs with per-agent config paths, formats, and gotchas
- Forage platform compatibility table with scheduling support per agent
- 134 tests across 3 layers (database, tools, MCP integration)
