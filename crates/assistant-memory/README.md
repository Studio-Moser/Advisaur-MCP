# assistant-memory

The reusable Advisaur memory engine: SQLite migrations through schema v18, FTS5 and vector search, typed temporal edges, project identity and scope, write reconciliation, trust fencing, curated briefs, repair, and all memory-domain tool handlers.

The crate opens the committed TypeScript-v18 compatibility fixture and existing user databases unchanged. It performs no inference; summaries, metadata, and optional embedding vectors come from callers.

```bash
cargo test -p assistant-memory
```

Applications can construct `Memory` with `Memory::open(path)` or `Memory::open_in_memory()` and reuse the domain modules without running an MCP transport.
