# ShelbyMCP

ShelbyMCP is a Rust workspace that ships a local knowledge-graph memory engine, an MCP server, reusable client integrations, and a dependency-free npm launcher.

## Related repositories

- Product strategy, ADRs and specs: `../Docs` (Studio-Moser/Advisaur-Docs) — `README.md` for intent, `Engineering/Decisions/0001-memory-server-architecture-contract.md` for the memory contract this workspace implements.
- Historical consumer: `../Apps` (Studio-Moser/Shelby-App, archived) is abandoned; its pinned crate embedding is not the active app architecture.
- Active client: `../MacOS` (Studio-Moser/Advisaur-MacOS), the self-contained native Swift/SwiftUI app with its own memory engine. Shelby-MCP remains a separate, related product. Interoperability is governed by Docs ADR-0009 and Advisaur-Docs#623; never assume a shared store or unsynchronized dual writers.

## Repository map

- `crates/assistant-memory/` — SQLite migrations, thoughts, search, vectors, edges, project identity, trust, repair, and tool-domain handlers.
- `crates/assistant-integrations/` — current client catalog and safe fallback configuration.
- `crates/assistant-mcp/` — MCP schemas, prompts, stdio/HTTP transports, OAuth, CLI, and public library surface.
- `bin/` and `npm/` — npm launcher and native platform package manifests.
- `integrations/` — source manifests for client-native packages; canonical skills are copied during assembly.
- `skills/` and `assets/` — canonical skill and prompt sources.
- `tests/fixtures/` — shared JSON fixtures and the immutable TypeScript-v18 SQLite compatibility fixture.
- `scripts/` — dependency-free package, workflow, and documentation checks.

## Required gates

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm ci
npm test
npm audit
```

For distribution changes, also build the binary and run the package checks documented in [Development](docs/DEVELOPMENT.md#distribution-checks).

## Invariants

1. The server performs no inference and never accepts model-provider credentials. Embeddings are caller-supplied vectors.
2. Existing schema-v18 databases must open without conversion. The committed TypeScript fixture is mandatory compatibility evidence.
3. Stdout is reserved for MCP JSON-RPC or the requested CLI payload. Diagnostics go to stderr.
4. Every MCP tool has static descriptions, bounded inputs/results, actionable errors, and all four tool annotations.
5. Project-scoped work resolves through immutable project UUIDs and aliases; ambiguous scope fails closed.
6. Untrusted memory text remains fenced at every retrieval boundary.
7. Client setup preserves malformed files byte-for-byte, preserves unrelated configuration, writes atomically, and never edits global instruction files.
8. `skills/shelby-forage` and `skills/shelby-onboard` are canonical. Generated client packages copy them byte-for-byte.
9. OAuth and bearer checks are security boundaries; changes require focused tests and full workspace verification.
10. Do not add a Node runtime dependency to the server or npm launcher.

## Implementation style

Prefer existing helpers and plain Rust. Keep SQL in the memory crate, client-specific configuration in the integrations crate, and transport concerns in the MCP crate. Add a focused test for every non-trivial branch or parser. Do not write user memory, credentials, generated archives, databases, or build output into the repository.
