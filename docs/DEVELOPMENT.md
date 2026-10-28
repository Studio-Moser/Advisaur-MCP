# Development

## Prerequisites

- Stable Rust with `rustfmt` and clippy
- Node.js 20 or newer and npm
- `zip` for integration package assembly

Node runs only repository scripts and the npm launcher tests. The server and memory implementation are Rust.

## Build and test

```bash
npm ci
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm test
npm audit
```

Useful focused commands:

```bash
cargo test -p assistant-memory
cargo test -p assistant-integrations
cargo test -p assistant-mcp --test cli -- --nocapture
cargo test -p assistant-mcp --test stdio -- --nocapture
node --test tests/launcher.test.js
node --test tests/integrations.test.js
```

The Rust workspace test includes a mandatory read/extend check against `tests/fixtures/TypeScript-v18.sqlite`. Do not regenerate that fixture: it is immutable evidence that the current engine opens databases created before the cutover.

## Run locally

```bash
cargo run -p assistant-mcp -- --db :memory:
cargo run -p assistant-mcp -- --transport http --host 127.0.0.1 --port 3100 --db :memory:
```

Use a temporary database for manual testing. Never point tests at a user's `~/.studiomoser-assistant-mcp/memory.db`.

## Repository structure

```text
crates/
  assistant-memory/       memory engine and domain handlers
  assistant-memory-eval/  deterministic contract and retrieval evaluation
  assistant-integrations/ client catalog and safe fallback setup
  assistant-mcp/          MCP library, HTTP/OAuth, and binary CLI
bin/                   dependency-free npm launcher
npm/                   five native package manifests
integrations/          client package source manifests
skills/                canonical Forage and Onboard skills
assets/                canonical printable prompts
scripts/               package and contract checks
tests/fixtures/         cross-engine and shared policy fixtures
```

Keep boundaries strict: SQL and memory rules belong in `assistant-memory`; client configuration belongs in `assistant-integrations`; protocol and transport code belongs in `advisaur-mcp`.

## Contract changes

Start with a failing focused test. Schema, persistence, authentication, project identity, trust, and package-format changes require full workspace verification. Preserve input bounds and actionable errors at trust boundaries. Tool descriptions must stay static because clients include them in prompt context.

When changing a shared contract, update the corresponding JSON fixture and every consumer in the same change. A schema migration must be additive, transactional, and tested from the prior schema as well as a fresh database.

## Distribution checks

Build and smoke-test the current native npm package:

```bash
cargo build -p assistant-mcp
node "scripts/Build Native Packages.mjs" \
  --target aarch64-apple-darwin \
  --binary target/debug/advisaur-mcp \
  --output target/npm
node "scripts/Verify Package.mjs" --binary target/debug/advisaur-mcp
```

Use the target triple matching your machine when building a platform package. Assemble and validate client sources with:

```bash
node "scripts/Build Integration Packages.mjs" --all --binary target/debug/advisaur-mcp
python3 ~/.codex/skills/.system/plugin-creator/scripts/validate_plugin.py integrations/codex/advisaur-mcp
claude plugin validate integrations/claude-code
```

The last two commands are optional local validators; repository contract tests remain authoritative in CI. Generated files belong under `target/` and must not be committed.

## Release safety

`scripts/Verify Release Workflow.mjs` checks the release policy locally. The workflow always builds and uploads dry-run artifacts, but publishes only when all of these are true:

1. The run is manually requested with `publish: true`.
2. The ref is the exact `v0.4.0` tag.
3. The protected `release` environment approves the job.

Platform npm packages publish before the wrapper. `assistant-memory` and `assistant-integrations` publish as reusable Rust libraries. No local command in the development gate publishes anything.
