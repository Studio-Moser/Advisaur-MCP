<p align="center">
</p>

<p align="center">
  <strong>Local knowledge-graph memory for AI tools, implemented as one Rust binary.</strong>
</p>

Advisaur MCP gives MCP-compatible agents durable memory across sessions. Thoughts live in a local SQLite database with FTS5 search, optional vectors, typed graph edges, project scope, trust fencing, and curated context briefs. The server performs no inference and makes no model API calls.

## Quick start

Run the platform-native binary through npm:

```bash
npx -y advisaur-mcp --version
npx -y advisaur-mcp
```

The default stdio server stores data at `~/.studiomoser-assistant-mcp/memory.db`. Existing databases created by Advisaur MCP schema v18 open unchanged.

The server was called ShelbyMCP before its rename (ADR-0012). An existing `~/.shelbymcp` folder moves to the new one the first time the server runs with its default database, and the old `SHELBY_DB_PATH`, `SHELBY_TRANSPORT` and `SHELBY_API_KEY` variables are still read, with a warning, for one release. `npx shelbymcp` still runs the server and says where it went.

Package-first integrations are included in the protected `v0.4.0` release:

| Client | Release package |
|---|---|
| ChatGPT / Codex | `advisaur-mcp-codex-0.4.0.zip` |
| Claude Code | `advisaur-mcp-claude-code-0.4.0.zip` |
| Cursor and Agent Plugins clients | `advisaur-mcp-agent-plugin-0.4.0.zip` |
| Gemini CLI | `advisaur-mcp-gemini-0.4.0.zip` |
| Antigravity | `advisaur-mcp-antigravity-0.4.0.zip` |
| Claude Desktop | `advisaur-mcp-claude-desktop-<platform>-0.4.0.mcpb` |
| Devin | `advisaur-mcp-devin-0.4.0.zip` |

Use `advisaur-mcp setup <client>` only when the client package or marketplace entry is not available yet. The fallback supports `claude-code`, `claude-desktop`, `cursor`, `codex`, `gemini`, and `antigravity`; `devin` directs you to its organization-managed marketplace, while `windsurf` keeps the legacy local config flow. It safely merges the MCP entry and never appends instructions to a global rules file. See [Agent Setup](docs/AGENT-SETUP.md).

## What agents can do

The server exposes 12 MCP tools:

- Capture, search, list, fetch, update, and delete thoughts.
- Link and traverse typed graph edges with `manage_edges`, `explore_graph`, and `expand_neighbors`.
- Build trusted, token-bounded context with `get_brief` and `select_context`.
- Audit the database with `thought_stats`.

Search returns summaries and IDs so clients can fetch full content only when needed. Project-aware operations resolve scope from MCP roots and fail closed when the project cannot be identified. Untrusted or external memories are returned inside explicit data-only fences.

The optional Forage and Onboard skills are packaged with supported clients. They run in the user's agent session; Advisaur MCP does not receive or store model credentials. Standalone prompt bodies remain available:

```bash
advisaur-mcp forage
advisaur-mcp onboard
advisaur-mcp migrate
advisaur-mcp protocol
```

## HTTP and OAuth

Start Streamable HTTP on port 3100:

```bash
ADVISAUR_API_KEY="replace-with-a-long-random-secret" \
  npx -y advisaur-mcp --transport http --host 127.0.0.1 --port 3100
```

Endpoints include `/mcp`, `/health`, and MCP discovery documents. Setting `ADVISAUR_API_KEY` enables bearer authentication and an OAuth authorization-code flow with protected-resource discovery, S256 PKCE, dynamic client registration, and resource-bound access and refresh tokens. Tokens do not expire or rotate; rotate `ADVISAUR_API_KEY` to revoke them. Without it, HTTP runs unauthenticated and the OAuth endpoints return `503`; do not bind an unauthenticated server to an untrusted network.

## Build from source

```bash
git clone https://github.com/Studio-Moser/Advisaur-MCP.git
cd Advisaur-MCP
cargo build -p assistant-mcp
cargo test --workspace
npm ci
npm test
```

Rust owns the product. Node is used only for the small npm launcher, packaging scripts, and their tests. See [Development](docs/DEVELOPMENT.md) and [Architecture](docs/ARCHITECTURE.md).

Memory-engine pull requests also run a deterministic contract and public retrieval gate. See [Memory evaluation](<docs/Memory Evaluation.md>).

## Reuse in an app

The workspace is layered so an app can embed it:

- `assistant-memory` is the SQLite memory engine and all memory-domain behavior.
- `assistant-integrations` is the current client catalog plus safe setup/status/uninstall helpers.
- `assistant-mcp` exposes the MCP service and reusable Axum router as a library as well as a binary.

The app can embed these crates in-process, share one database contract across macOS and iOS, and host the same HTTP router without spawning Node.

## License

MIT. See [LICENSE](LICENSE).
