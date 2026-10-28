# Contributing to Advisaur MCP

Open an issue before a substantial change so the behavior and compatibility contract can be agreed before implementation.

## Development setup

Install stable Rust and Node.js 20 or newer, then run:

```bash
git clone https://github.com/Studio-Moser/Advisaur-MCP.git
cd Advisaur-MCP
npm ci
cargo test --workspace
npm test
```

Rust is the product implementation. Node is used only for the npm launcher and packaging checks.

## Pull requests

1. Branch from `main` and keep the change focused.
2. Add a test that demonstrates new behavior or the fixed failure.
3. Preserve schema-v18 and MCP contract compatibility unless the change explicitly includes a migration and contract update.
4. Run `cargo fmt --check`, clippy with warnings denied, all Rust tests, `npm test`, and `npm audit`.
5. Update user-facing documentation when commands, packages, or behavior change.
6. Fill out the pull request template and link the issue.

See [Development](docs/DEVELOPMENT.md) for package-specific commands and repository structure.

## Code style

- Follow `rustfmt` and keep clippy clean with `-D warnings`.
- Use explicit error types and preserve context without exposing secrets.
- Keep stdout protocol-safe; send operational logs to stderr.
- Prefer table-driven tests for contracts and fixtures for cross-client behavior.
- Do not add inference, model authentication, or implicit writes to global agent instructions.

Contributions are licensed under the [MIT License](LICENSE).
