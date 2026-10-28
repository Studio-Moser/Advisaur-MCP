# Agent Setup

Advisaur MCP ships package-native integration sources for current AI clients. Each package starts the same local command:

```json
{
  "command": "npx",
  "args": ["-y", "advisaur-mcp"]
}
```

Prefer the package for your client because it includes the canonical Forage and Onboard skills without modifying global instruction files.

## Package routes

| Client | Artifact | Installation route |
|---|---|---|
| ChatGPT / Codex | `advisaur-mcp-codex-0.4.0.zip` | Codex plugin marketplace package; use the CLI fallback until its marketplace listing is available. |
| Claude Code | `advisaur-mcp-claude-code-0.4.0.zip` | Claude Code marketplace package; use the CLI fallback until its listing is available. |
| Cursor | `advisaur-mcp-agent-plugin-0.4.0.zip` | Agent Plugins 1.0 package for the client plugin UI or registry. |
| Gemini CLI | `advisaur-mcp-gemini-0.4.0.zip` | Unpack, then run `gemini extensions install <unpacked-directory>`. |
| Antigravity | `advisaur-mcp-antigravity-0.4.0.zip` | Unpack as `advisaur-mcp` under `~/.gemini/config/plugins/`. |
| Claude Desktop | `advisaur-mcp-claude-desktop-<platform>-0.4.0.mcpb` | Choose the artifact matching your OS and architecture, then open it in Claude Desktop. |
| Devin | `advisaur-mcp-devin-0.4.0.zip` | In Settings > MCP Marketplace, choose Add Your Own and use its `registry.json` values. |

Release artifacts are checksummed in `SHA256SUMS`. Review a package's MCP command and requested capabilities before installing it.

## CLI fallback

Install the npm wrapper globally or invoke it with `npx`:

```bash
npm install -g advisaur-mcp
advisaur-mcp setup cursor
```

Supported client names are:

```text
claude-code  claude-desktop  cursor  codex  devin  gemini  antigravity
```

`devin` returns exit code `2` with its organization-managed MCP Marketplace route; there is no local Devin config file to edit. `windsurf` remains a separate compatibility command for the legacy local Windsurf config. Setup prefers the client's own CLI when that is the safe current route. JSON fallbacks merge only `mcpServers.advisaur-mcp` (replacing an entry the server had under its old name, `shelbymcp`), preserve unrelated keys and file permissions, use atomic replacement, and leave malformed files byte-identical with manual instructions. Exit code `2` means manual action is required.

Remove a fallback entry with:

```bash
advisaur-mcp uninstall cursor
```

## Manual MCP configuration

If neither a package nor fallback installer is usable, add this server entry using the client's MCP settings:

```json
{
  "mcpServers": {
    "advisaur-mcp": {
      "command": "npx",
      "args": ["-y", "advisaur-mcp"]
    }
  }
}
```

Do not put `npx -y advisaur-mcp` into a single `command` string. Keep the executable and arguments separate. On native Windows JSON fallbacks, use `"command": "cmd"` with `"args": ["/c", "npx", "-y", "advisaur-mcp"]` so clients can launch npm's command shim.

## Skills and cold start

Packages for skill-capable clients include `advisaur-forage` and `advisaur-onboard` byte-for-byte from the repository's canonical `skills/` directory. For clients that do not expose packaged skills, print the prompt and paste it into a conversation or scheduled task:

```bash
advisaur-mcp onboard
advisaur-mcp forage
```

To migrate context from another assistant, run `advisaur-mcp migrate`, paste the output into that assistant, and give its structured response to an Advisaur-connected agent.

## Verify

Restart the client after installation, inspect its MCP server status, and ask it to list Advisaur's tools. The server should expose 12 tools. Capture one test thought, start a new session, and search for it.

For direct protocol verification from source, run `cargo test -p assistant-mcp --test stdio`.
