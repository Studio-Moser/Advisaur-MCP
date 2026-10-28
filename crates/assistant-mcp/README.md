# assistant-mcp

The Rust MCP service and command line for Shelby memory. The crate exposes the same server as a binary and a reusable library.

```bash
cargo run -p assistant-mcp -- --db ~/.shelbymcp/memory.db
cargo run -p assistant-mcp -- --transport http --host 127.0.0.1 --port 3100
```

Public integration points include `open_memory`, `server::MemoryServer`, and `http::router`. This lets Shelby App embed the service in-process while standalone clients use stdio or Streamable HTTP.

HTTP serves `/mcp`, `/health`, discovery documents, bearer authentication, and OAuth 2.1 when `SHELBY_API_KEY` is set. The CLI also provides safe client setup/uninstall, prompt printers, and project-identity repair.

```bash
cargo test -p assistant-mcp
```
