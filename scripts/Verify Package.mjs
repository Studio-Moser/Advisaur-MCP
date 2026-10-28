import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import { buildNativePackage } from "./Build Native Packages.mjs";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const CURRENT_TARGETS = new Map([
  ["darwin-arm64", "aarch64-apple-darwin"],
  ["darwin-x64", "x86_64-apple-darwin"],
  ["linux-arm64", "aarch64-unknown-linux-gnu"],
  ["linux-x64", "x86_64-unknown-linux-gnu"],
  ["win32-x64", "x86_64-pc-windows-msvc"],
]);

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: "utf8",
    shell: false,
    ...options,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} failed (${result.status}): ${result.stderr || result.stdout}`);
  }
  return result;
}

function parseBinary(argv) {
  if (argv.length !== 2 || argv[0] !== "--binary" || !argv[1]) {
    throw new Error("Usage: Verify Package.mjs --binary <path>");
  }
  return argv[1];
}

export function verifyPackage(binary) {
  const target = CURRENT_TARGETS.get(`${process.platform}-${process.arch}`);
  if (!target) throw new Error(`Unsupported verification platform: ${process.platform}-${process.arch}`);

  const temporaryRoot = mkdtempSync(join(tmpdir(), "advisaur-mcp-verify-"));
  try {
    const archives = join(temporaryRoot, "archives");
    const prefix = join(temporaryRoot, "install");
    mkdirSync(archives, { recursive: true });
    mkdirSync(prefix, { recursive: true });

    const nativeTarball = buildNativePackage({ target, binary, output: archives });
    const packedWrapper = JSON.parse(run("npm", [
      "pack",
      repositoryRoot,
      "--pack-destination",
      archives,
      "--json",
    ]).stdout);
    if (!Array.isArray(packedWrapper) || packedWrapper.length !== 1 || !packedWrapper[0].filename) {
      throw new Error("npm pack did not report exactly one wrapper package");
    }
    const wrapperTarball = join(archives, packedWrapper[0].filename);

    run("npm", [
      "install",
      "--prefix",
      prefix,
      "--ignore-scripts",
      "--no-audit",
      "--no-fund",
      wrapperTarball,
      nativeTarball,
    ]);

    const executable = process.platform === "win32"
      ? join(prefix, "node_modules", ".bin", "advisaur-mcp.cmd")
      : join(prefix, "node_modules", ".bin", "advisaur-mcp");
    const version = run(executable, ["--version"]);
    if (version.stdout.trim() !== "advisaur-mcp v0.4.0") {
      throw new Error(`Unexpected packaged version: ${version.stdout.trim()}`);
    }
    return version.stdout.trim();
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
}

const isEntryPoint = process.argv[1]
  && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));
if (isEntryPoint) {
  try {
    process.stdout.write(`${verifyPackage(parseBinary(process.argv.slice(2)))}\n`);
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
