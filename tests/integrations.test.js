import assert from "node:assert/strict";
import { existsSync, lstatSync, mkdtempSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

import { buildIntegrationPackages, safeCopyTree } from "../scripts/Build Integration Packages.mjs";

const root = resolve(import.meta.dirname, "..");
const readJson = (path) => JSON.parse(readFileSync(join(root, path), "utf8"));
const server = { command: "npx", args: ["-y", "shelbymcp"] };

test("source manifests match each current client contract", () => {
  const agent = readJson("integrations/agent-plugin/plugin.json");
  assert.equal(agent.$schema, "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json");
  assert.equal(agent.name, "shelbymcp");
  assert.equal(agent.version, "0.4.0");
  const agentMcp = readJson("integrations/agent-plugin/mcp.json");
  assert.equal(agentMcp.$schema, "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json");
  assert.deepEqual(agentMcp.mcpServers.shelbymcp, { type: "stdio", ...server });

  const codex = readJson("integrations/codex/shelbymcp/.codex-plugin/plugin.json");
  assert.equal(codex.name, "shelbymcp");
  assert.equal(codex.version, "0.4.0");
  assert.deepEqual(readJson("integrations/codex/shelbymcp/.mcp.json").mcpServers.shelbymcp, server);

  const claude = readJson("integrations/claude-code/.claude-plugin/plugin.json");
  assert.equal(claude.version, "0.4.0");
  assert.deepEqual(readJson("integrations/claude-code/.mcp.json").mcpServers.shelbymcp, server);

  const gemini = readJson("integrations/gemini/gemini-extension.json");
  assert.equal(gemini.version, "0.4.0");
  assert.deepEqual(gemini.mcpServers.shelbymcp, server);

  const antigravity = readJson("integrations/antigravity/plugin.json");
  assert.equal(antigravity.version, "0.4.0");
  assert.deepEqual(readJson("integrations/antigravity/mcp_config.json").mcpServers.shelbymcp, server);

  const mcpb = readJson("integrations/claude-desktop/manifest.json");
  assert.equal(mcpb.manifest_version, "0.4");
  assert.equal(mcpb.version, "0.4.0");
  assert.equal(mcpb.server.type, "binary");
  assert.equal(mcpb.server.entry_point, "server/advisaur-mcp");

  const devin = readJson("integrations/devin/registry.json");
  assert.equal(devin.version, "0.4.0");
  assert.deepEqual(devin.defaultConfiguration, { transport: "STDIO", ...server });
  assert.equal(devin.installation, "Settings > MCP Marketplace > Add Your Own");
  assert.equal(devin.compatibility.clientAlias, "windsurf");
});

test("assembly copies canonical skills byte-for-byte and builds a platform-specific MCPB", () => {
  const temporary = mkdtempSync(join(tmpdir(), "shelbymcp-integrations-"));
  const binary = join(temporary, "advisaur-mcp");
  writeFileSync(binary, "fake native binary");
  const output = join(temporary, "output");

  buildIntegrationPackages({ clients: ["all"], binary, output });

  for (const client of ["agent-plugin", "codex", "claude-code", "gemini", "antigravity"]) {
    for (const skill of ["shelby-forage", "shelby-onboard"]) {
      assert.equal(
        readFileSync(join(output, client, "skills", skill, "SKILL.md"), "utf8"),
        readFileSync(join(root, "skills", skill, "SKILL.md"), "utf8"),
      );
    }
  }
  const platform = `${process.platform}-${process.arch}`;
  const packageRoot = join(output, `claude-desktop-${platform}`);
  assert.equal(
    readFileSync(join(packageRoot, "server", "advisaur-mcp"), "utf8"),
    "fake native binary",
  );
  const packagedManifest = JSON.parse(readFileSync(join(packageRoot, "manifest.json"), "utf8"));
  assert.deepEqual(packagedManifest.compatibility.platforms, [process.platform]);
  assert.ok(existsSync(join(output, `shelbymcp-claude-desktop-${platform}-0.4.0.mcpb`)));
  assert.equal(lstatSync(join(packageRoot, "server", "advisaur-mcp")).isSymbolicLink(), false);
});

test("assembly names Windows MCPB binaries with the executable suffix", () => {
  const temporary = mkdtempSync(join(tmpdir(), "shelbymcp-windows-mcpb-"));
  const binary = join(temporary, "advisaur-mcp.exe");
  writeFileSync(binary, "fake Windows binary");
  const output = join(temporary, "output");

  buildIntegrationPackages({
    clients: ["claude-desktop"],
    mcpb: [{ target: "x86_64-pc-windows-msvc", binary }],
    output,
  });

  const packageRoot = join(output, "claude-desktop-win32-x64");
  const manifest = JSON.parse(readFileSync(join(packageRoot, "manifest.json"), "utf8"));
  assert.equal(manifest.server.entry_point, "server/advisaur-mcp.exe");
  assert.equal(manifest.server.mcp_config.command, "${__dirname}/server/advisaur-mcp.exe");
  assert.ok(existsSync(join(packageRoot, "server", "advisaur-mcp.exe")));
});

test("assembly rejects unknown clients and source symlinks", () => {
  const temporary = mkdtempSync(join(tmpdir(), "shelbymcp-integrations-safety-"));
  const binary = join(temporary, "advisaur-mcp");
  writeFileSync(binary, "binary");
  assert.throws(
    () => buildIntegrationPackages({ clients: ["unknown"], binary, output: join(temporary, "out") }),
    /Unknown integration client/,
  );

  const source = join(temporary, "source");
  const outside = join(temporary, "outside");
  writeFileSync(outside, "private");
  symlinkSync(outside, source);
  assert.throws(() => safeCopyTree(source, join(temporary, "copy")), /symbolic link/);
});
