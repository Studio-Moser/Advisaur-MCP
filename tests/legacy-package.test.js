import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

const root = new URL("..", import.meta.url).pathname;
const readJson = (path) => JSON.parse(readFileSync(join(root, path), "utf8"));

test("the last shelbymcp release runs the renamed package at the same version", () => {
  const renamed = readJson("package.json");
  const legacy = readJson("deprecated/shelbymcp/package.json");
  assert.equal(renamed.name, "advisaur-mcp");
  assert.equal(legacy.name, "shelbymcp");
  assert.equal(legacy.version, renamed.version);
  assert.deepEqual(legacy.dependencies, { "advisaur-mcp": renamed.version });
  assert.equal(legacy.bin.shelbymcp, "shelbymcp.js");
  // It imports the renamed launcher, which the renamed package publishes.
  const wrapper = readFileSync(join(root, "deprecated/shelbymcp/shelbymcp.js"), "utf8");
  assert.match(wrapper, /from "advisaur-mcp\/bin\/advisaur-mcp\.js"/);
  assert.ok(renamed.files.includes("bin"));
  assert.equal(renamed.bin["advisaur-mcp"], "bin/advisaur-mcp.js");
  assert.equal(renamed.exports, undefined, "a subpath import needs no exports map, or one that allows it");
});

test("the npm package and the MCP Registry entry carry the server's name (ADR-0012 D1)", () => {
  const renamed = readJson("package.json");
  const server = readJson("server.json");
  assert.equal(renamed.name, "advisaur-mcp");
  assert.equal(renamed.mcpName, "io.github.Studio-Moser/advisaur-mcp");
  assert.equal(server.name, renamed.mcpName);
  assert.equal(server.version, renamed.version);
  for (const entry of server.packages) {
    assert.equal(entry.identifier, renamed.name);
    assert.equal(entry.version, renamed.version);
    assert.deepEqual(entry.runtimeArguments, [{ type: "positional", value: renamed.name }]);
  }
  for (const [platform, version] of Object.entries(renamed.optionalDependencies)) {
    assert.match(platform, /^advisaur-mcp-(darwin|linux|win32)-(arm64|x64)$/);
    assert.equal(version, renamed.version);
  }
});
