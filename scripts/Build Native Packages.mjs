import { spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const TARGETS = new Map([
  ["aarch64-apple-darwin", {
    packageDir: "darwin-arm64", executable: "advisaur-mcp", os: "darwin", cpu: "arm64",
  }],
  ["x86_64-apple-darwin", {
    packageDir: "darwin-x64", executable: "advisaur-mcp", os: "darwin", cpu: "x64",
  }],
  ["aarch64-unknown-linux-gnu", {
    packageDir: "linux-arm64", executable: "advisaur-mcp", os: "linux", cpu: "arm64",
  }],
  ["x86_64-unknown-linux-gnu", {
    packageDir: "linux-x64", executable: "advisaur-mcp", os: "linux", cpu: "x64",
  }],
  ["x86_64-pc-windows-msvc", {
    packageDir: "win32-x64", executable: "advisaur-mcp.exe", os: "win32", cpu: "x64",
  }],
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
  return result.stdout;
}

export function buildNativePackage({ target, binary, output }) {
  const definition = TARGETS.get(target);
  if (!definition) throw new Error(`Unsupported Rust target: ${target}`);

  const binaryPath = resolve(binary);
  if (!existsSync(binaryPath)) throw new Error(`Native binary does not exist: ${binaryPath}`);

  const outputDir = resolve(output);
  mkdirSync(outputDir, { recursive: true });
  const temporaryRoot = mkdtempSync(join(tmpdir(), "shelbymcp-native-"));
  try {
    const packageRoot = join(temporaryRoot, definition.packageDir);
    const binDir = join(packageRoot, "bin");
    mkdirSync(binDir, { recursive: true });

    const sourceManifest = join(repositoryRoot, "npm", definition.packageDir, "package.json");
    const manifest = JSON.parse(readFileSync(sourceManifest, "utf8"));
    manifest.os = [definition.os];
    manifest.cpu = [definition.cpu];
    writeFileSync(join(packageRoot, "package.json"), `${JSON.stringify(manifest, null, 2)}\n`);
    copyFileSync(join(repositoryRoot, "LICENSE"), join(packageRoot, "LICENSE"));

    const destinationBinary = join(binDir, definition.executable);
    copyFileSync(binaryPath, destinationBinary);
    if (definition.executable !== "advisaur-mcp.exe") chmodSync(destinationBinary, 0o755);

    const packed = JSON.parse(run("npm", [
      "pack",
      packageRoot,
      "--pack-destination",
      outputDir,
      "--json",
    ]));
    if (!Array.isArray(packed) || packed.length !== 1 || !packed[0].filename) {
      throw new Error("npm pack did not report exactly one package");
    }
    return join(outputDir, basename(packed[0].filename));
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
}

function parseArgs(argv) {
  const values = {};
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!["--target", "--binary", "--output"].includes(flag) || !value) {
      throw new Error("Usage: Build Native Packages.mjs --target <triple> --binary <path> --output <dir>");
    }
    values[flag.slice(2)] = value;
  }
  if (!values.target || !values.binary || !values.output) {
    throw new Error("Usage: Build Native Packages.mjs --target <triple> --binary <path> --output <dir>");
  }
  return values;
}

const isEntryPoint = process.argv[1]
  && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));
if (isEntryPoint) {
  try {
    process.stdout.write(`${buildNativePackage(parseArgs(process.argv.slice(2)))}\n`);
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
