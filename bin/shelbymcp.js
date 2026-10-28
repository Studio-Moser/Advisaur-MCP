#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { realpathSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve as resolvePath } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const PLATFORM_PACKAGES = new Map([
  ["darwin-arm64", "shelbymcp-darwin-arm64"],
  ["darwin-x64", "shelbymcp-darwin-x64"],
  ["linux-arm64", "shelbymcp-linux-arm64"],
  ["linux-x64", "shelbymcp-linux-x64"],
  ["win32-x64", "shelbymcp-win32-x64"],
]);

const SIGNAL_NUMBERS = {
  SIGHUP: 1,
  SIGINT: 2,
  SIGQUIT: 3,
  SIGABRT: 6,
  SIGKILL: 9,
  SIGUSR1: 10,
  SIGUSR2: 12,
  SIGPIPE: 13,
  SIGALRM: 14,
  SIGTERM: 15,
};

const require = createRequire(import.meta.url);

export function selectPackage(platform, arch) {
  const packageName = PLATFORM_PACKAGES.get(`${platform}-${arch}`);
  if (!packageName) {
    throw new Error(`Unsupported platform: ${platform}-${arch}`);
  }
  return packageName;
}

export function resolveBinary(packageName, resolver = require.resolve, platform = process.platform) {
  let manifest;
  try {
    manifest = resolver(`${packageName}/package.json`);
  } catch (error) {
    throw new Error(
      `npm install did not include ${packageName}; reinstall shelbymcp for this platform`,
      { cause: error },
    );
  }
  const executable = platform === "win32" ? "advisaur-mcp.exe" : "advisaur-mcp";
  return join(dirname(manifest), "bin", executable);
}

export function launch(argv, dependencies = {}) {
  const platform = dependencies.platform ?? process.platform;
  const arch = dependencies.arch ?? process.arch;
  const resolver = dependencies.resolve ?? require.resolve;
  const run = dependencies.spawnSync ?? spawnSync;
  const writeError = dependencies.writeError ?? ((message) => process.stderr.write(message));
  const signalNumbers = dependencies.signalNumbers ?? SIGNAL_NUMBERS;

  try {
    const packageName = selectPackage(platform, arch);
    const binary = resolveBinary(packageName, resolver, platform);
    const result = run(binary, argv, { stdio: "inherit", shell: false });
    if (result.error) {
      writeError(`shelbymcp: ${result.error.message}\n`);
      return 1;
    }
    if (result.signal) {
      return 128 + (signalNumbers[result.signal] ?? 0);
    }
    return result.status ?? 1;
  } catch (error) {
    writeError(`shelbymcp: ${error.message}\n`);
    return 1;
  }
}

function isSameFile(left, right) {
  try {
    return realpathSync(left) === realpathSync(right);
  } catch {
    return resolvePath(left) === resolvePath(right);
  }
}

const isEntryPoint = process.argv[1]
  && isSameFile(process.argv[1], fileURLToPath(import.meta.url));
if (isEntryPoint) {
  process.exitCode = launch(process.argv.slice(2));
}
