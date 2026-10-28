# Architecture

ShelbyMCP is one Rust implementation shared by the standalone server and future Shelby App hosts.

```text
AI client or Shelby App
        │
        ├─ stdio MCP ───────────────┐
        ├─ Streamable HTTP / OAuth ─┤
        └─ in-process Rust API ─────┤
                                    ▼
                              assistant-mcp
                                    │
                       ┌────────────┴────────────┐
                       ▼                         ▼
             assistant-integrations          assistant-memory
             client catalog/setup       SQLite + search + graph
```

## Crate boundaries

### `assistant-memory`

Owns persistence and memory-domain behavior: migrations, thought CRUD, FTS5, optional float-vector search, temporal graph edges, project registry and aliases, scope resolution, reconciliation, trust fencing, telemetry, project repair, context selection, and brief policy. `Memory::open` migrates databases through schema v18.

The committed `tests/fixtures/TypeScript-v18.sqlite` file is the compatibility boundary with installations created before the Rust cutover. The Rust cross-engine test must always open and extend it; absence is a test failure.

### `assistant-integrations`

Owns the seven-client catalog and safe fallback setup/status/uninstall behavior. It uses a client's supported CLI when available, otherwise performs a narrow JSON merge where that format is current. Writes are atomic, preserve existing permissions and unrelated keys, and stop for malformed files. The library never writes global rules or memory-protocol text.

### `assistant-mcp`

Owns the MCP boundary: static schemas and annotations for 12 tools, prompts/resources/completions/logging, stdio, Streamable HTTP, OAuth, CLI parsing, and reusable service construction. `server::MemoryServer` and `http::router` are public so an application can host exactly the same protocol surface without launching a subprocess.

## Data and retrieval

Thoughts use UUID primary keys and carry content, a short summary, type, source, optional caller-supplied embedding, topics, people, visibility, trust level, timestamps, and project identity. FTS5 indexes text. Vector search runs only when a caller supplies vectors; the server does not generate them.

Typed edges are `refines`, `cites`, `refuted_by`, `tags`, `related`, and `follows`. Temporal validity supports expiring facts without deleting history. Capture reconciliation can create, reinforce, merge, or return supersession suggestions.

Search and list operations return bounded summary results. Full content requires `get_thought`. `get_brief` applies the deterministic privacy/trust policy and a token budget; `select_context` provides targeted filtering. Untrusted and external text is fenced at retrieval boundaries so it remains data rather than agent instruction.

## Project identity

Canonical project IDs are stable UUIDv5 values derived from normalized repository identity. Human-readable slugs remain aliases. Scope resolution uses explicit IDs first, then registered MCP roots and the longest matching member path. Ambiguous or missing scope fails closed: personal captures are rejected and reads use shared-only scope unless the caller explicitly requests all projects.

## Transports and authentication

Stdio is the default and reserves stdout for JSON-RPC. Streamable HTTP exposes `/mcp`, `/health`, and discovery endpoints. Local sessions are managed in-process by `rmcp`.

When `SHELBY_API_KEY` is configured, every `/mcp` request requires a valid bearer token. The same secret enables OAuth authorization-server and protected-resource metadata, dynamic client registration, authorization code with S256 PKCE, and resource-bound HMAC-derived access and refresh tokens. Client registrations are stored in SQLite; authorization codes and rate-limit state are in memory. Tokens do not expire or rotate, so rotating `SHELBY_API_KEY` revokes them. When no secret is configured, OAuth endpoints return `503` and HTTP is unauthenticated.

## Distribution

The root npm package is a dependency-free launcher. It maps the current OS/architecture to one of five optional native packages and calls the binary with `spawnSync`, inherited stdio, and `shell: false`. Platform tarballs declare exact `os` and `cpu` guards.

Client manifests live in `integrations/`. Assembly copies canonical skills and builds a target-labeled Claude Desktop MCPB from each explicit native binary. Release builds produce native archives, npm packages, client packages, Rust library crates, and SHA-256 checksums. Publication is disabled by default and requires the exact version tag plus approval through GitHub's `release` environment.

## Shelby App reuse

Desktop targets can host `assistant_mcp::http::router` on the app's chosen loopback port and share an `Arc<Mutex<Memory>>`. All app targets can call `assistant-memory` directly. The connections UI can use `assistant-integrations::CLIENTS` and its status/setup helpers. This preserves one memory engine and one installer catalog across the app and standalone product; iOS does not need a Node sidecar or subprocess.
