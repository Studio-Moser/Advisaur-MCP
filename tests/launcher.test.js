import assert from "node:assert/strict";
import test from "node:test";

import { launch, resolveBinary, selectPackage } from "../bin/advisaur-mcp.js";

const mappings = [
  ["darwin", "arm64", "advisaur-mcp-darwin-arm64", "advisaur-mcp"],
  ["darwin", "x64", "advisaur-mcp-darwin-x64", "advisaur-mcp"],
  ["linux", "arm64", "advisaur-mcp-linux-arm64", "advisaur-mcp"],
  ["linux", "x64", "advisaur-mcp-linux-x64", "advisaur-mcp"],
  ["win32", "x64", "advisaur-mcp-win32-x64", "advisaur-mcp.exe"],
];

test("maps every supported platform and architecture", () => {
  for (const [platform, arch, packageName] of mappings) {
    assert.equal(selectPackage(platform, arch), packageName);
  }
  assert.throws(() => selectPackage("freebsd", "x64"), /Unsupported platform/);
  assert.throws(() => selectPackage("darwin", "ia32"), /Unsupported platform/);
});

test("resolves the executable beside the platform package manifest", () => {
  for (const [platform, , packageName, executable] of mappings) {
    assert.equal(
      resolveBinary(packageName, () => `/modules/${packageName}/package.json`, platform),
      `/modules/${packageName}/bin/${executable}`,
    );
  }
});

test("reports a missing optional platform package", () => {
  assert.throws(
    () => resolveBinary("advisaur-mcp-linux-x64", () => {
      throw Object.assign(new Error("not found"), { code: "MODULE_NOT_FOUND" });
    }, "linux"),
    /npm install did not include advisaur-mcp-linux-x64/,
  );
});

test("forwards arguments without a shell and propagates the exit status", () => {
  let spawnCall;
  const status = launch(["serve", "--verbose"], {
    platform: "darwin",
    arch: "arm64",
    resolve: () => "/modules/advisaur-mcp-darwin-arm64/package.json",
    spawnSync(binary, argv, options) {
      spawnCall = { binary, argv, options };
      return { status: 7, signal: null };
    },
  });

  assert.deepEqual(spawnCall, {
    binary: "/modules/advisaur-mcp-darwin-arm64/bin/advisaur-mcp",
    argv: ["serve", "--verbose"],
    options: { stdio: "inherit", shell: false },
  });
  assert.equal(status, 7);
});

test("converts a terminating signal to the conventional exit status", () => {
  assert.equal(launch([], {
    platform: "linux",
    arch: "x64",
    resolve: () => "/modules/advisaur-mcp-linux-x64/package.json",
    spawnSync: () => ({ status: null, signal: "SIGTERM" }),
    signalNumbers: { SIGTERM: 15 },
  }), 143);
});

test("reports spawn failures and returns one", () => {
  const errors = [];
  assert.equal(launch([], {
    platform: "linux",
    arch: "x64",
    resolve: () => "/modules/advisaur-mcp-linux-x64/package.json",
    spawnSync: () => ({ error: new Error("cannot execute"), status: null, signal: null }),
    writeError: (message) => errors.push(message),
  }), 1);
  assert.match(errors.join(""), /cannot execute/);
});
