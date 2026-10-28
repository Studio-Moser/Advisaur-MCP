import { spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, join, parse, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const canonicalSkills = ["shelby-forage", "shelby-onboard"];
const packageClients = ["agent-plugin", "codex", "claude-code", "gemini", "antigravity"];
const allClients = [...packageClients, "claude-desktop", "devin"];
const sourceRoots = {
  "agent-plugin": join(repositoryRoot, "integrations", "agent-plugin"),
  codex: join(repositoryRoot, "integrations", "codex", "shelbymcp"),
  "claude-code": join(repositoryRoot, "integrations", "claude-code"),
  gemini: join(repositoryRoot, "integrations", "gemini"),
  antigravity: join(repositoryRoot, "integrations", "antigravity"),
  devin: join(repositoryRoot, "integrations", "devin"),
};
const mcpbTargets = new Map([
  ["aarch64-apple-darwin", { slug: "darwin-arm64", platform: "darwin", executable: "advisaur-mcp" }],
  ["x86_64-apple-darwin", { slug: "darwin-x64", platform: "darwin", executable: "advisaur-mcp" }],
  ["aarch64-unknown-linux-gnu", { slug: "linux-arm64", platform: "linux", executable: "advisaur-mcp" }],
  ["x86_64-unknown-linux-gnu", { slug: "linux-x64", platform: "linux", executable: "advisaur-mcp" }],
  ["x86_64-pc-windows-msvc", { slug: "win32-x64", platform: "win32", executable: "advisaur-mcp.exe" }],
]);
const hostTargets = new Map([
  ["darwin-arm64", "aarch64-apple-darwin"],
  ["darwin-x64", "x86_64-apple-darwin"],
  ["linux-arm64", "aarch64-unknown-linux-gnu"],
  ["linux-x64", "x86_64-unknown-linux-gnu"],
  ["win32-x64", "x86_64-pc-windows-msvc"],
]);
const archiveTimestamp = new Date("2000-01-01T00:00:00Z");

export function safeCopyTree(source, destination) {
  const metadata = lstatSync(source);
  if (metadata.isSymbolicLink()) {
    throw new Error(`Refusing to copy symbolic link: ${source}`);
  }
  if (metadata.isDirectory()) {
    mkdirSync(destination, { recursive: true });
    for (const entry of readdirSync(source).sort()) {
      safeCopyTree(join(source, entry), join(destination, entry));
    }
    return;
  }
  if (!metadata.isFile()) throw new Error(`Unsupported integration source entry: ${source}`);
  mkdirSync(dirname(destination), { recursive: true });
  copyFileSync(source, destination);
}

function copySkills(destination) {
  for (const skill of canonicalSkills) {
    safeCopyTree(
      join(repositoryRoot, "skills", skill),
      join(destination, "skills", skill),
    );
  }
}

function normalizeTimestamps(path) {
  const metadata = lstatSync(path);
  if (metadata.isDirectory()) {
    for (const entry of readdirSync(path).sort()) normalizeTimestamps(join(path, entry));
  }
  utimesSync(path, archiveTimestamp, archiveTimestamp);
}

function buildMcpb(target, binary, outputRoot) {
  const definition = mcpbTargets.get(target);
  if (!definition) throw new Error(`Unsupported Claude Desktop target: ${target}`);
  if (!binary) throw new Error("Claude Desktop packaging requires --binary <path>");
  const binaryPath = resolve(binary);
  if (!existsSync(binaryPath)) throw new Error(`Native binary does not exist: ${binaryPath}`);

  const packageRoot = join(outputRoot, `claude-desktop-${definition.slug}`);
  safeCopyTree(join(repositoryRoot, "integrations", "claude-desktop"), packageRoot);
  const manifestPath = join(packageRoot, "manifest.json");
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  manifest.server.entry_point = `server/${definition.executable}`;
  manifest.server.mcp_config.command = `\${__dirname}/server/${definition.executable}`;
  manifest.compatibility = { platforms: [definition.platform] };
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  const serverDir = join(packageRoot, "server");
  mkdirSync(serverDir, { recursive: true });
  const destination = join(serverDir, definition.executable);
  copyFileSync(binaryPath, destination);
  chmodSync(destination, 0o755);
  normalizeTimestamps(packageRoot);

  const archive = join(
    outputRoot,
    `shelbymcp-claude-desktop-${definition.slug}-${manifest.version}.mcpb`,
  );
  const packed = spawnSync("zip", ["-X", "-q", "-r", archive, "manifest.json", "server"], {
    cwd: packageRoot,
    encoding: "utf8",
    shell: false,
  });
  if (packed.error) throw packed.error;
  if (packed.status !== 0) throw new Error(`zip failed (${packed.status}): ${packed.stderr}`);
}

export function buildIntegrationPackages({ clients, binary, mcpb, output }) {
  const requested = clients.includes("all") ? allClients : [...new Set(clients)];
  if (requested.length === 0) throw new Error("Choose --all or at least one --client");
  for (const client of requested) {
    if (!allClients.includes(client)) throw new Error(`Unknown integration client: ${client}`);
  }

  const outputRoot = resolve(output);
  if (outputRoot === parse(outputRoot).root || outputRoot === repositoryRoot) {
    throw new Error(`Unsafe integration output path: ${outputRoot}`);
  }
  rmSync(outputRoot, { recursive: true, force: true });
  mkdirSync(outputRoot, { recursive: true });

  let mcpbInputs = mcpb;
  if (!mcpbInputs && binary) {
    const target = hostTargets.get(`${process.platform}-${process.arch}`);
    if (!target) throw new Error(`Unsupported Claude Desktop host: ${process.platform}-${process.arch}`);
    mcpbInputs = [{ target, binary }];
  }

  for (const client of requested) {
    if (client === "claude-desktop") {
      if (!mcpbInputs?.length) {
        throw new Error("Claude Desktop packaging requires --binary or --mcpb");
      }
      for (const input of mcpbInputs) buildMcpb(input.target, input.binary, outputRoot);
      continue;
    }
    const destination = join(outputRoot, client);
    safeCopyTree(sourceRoots[client], destination);
    if (packageClients.includes(client)) copySkills(destination);
  }
  return requested.map((client) => join(outputRoot, client));
}

function parseArgs(argv) {
  const clients = [];
  let binary;
  const mcpb = [];
  for (let index = 0; index < argv.length; index += 1) {
    switch (argv[index]) {
      case "--all":
        clients.push("all");
        break;
      case "--client":
        clients.push(argv[++index]);
        break;
      case "--binary":
        binary = argv[++index];
        break;
      case "--mcpb": {
        const value = argv[++index];
        const separator = value?.indexOf("=") ?? -1;
        if (separator <= 0 || separator === value.length - 1) {
          throw new Error("--mcpb requires <target>=<binary-path>");
        }
        mcpb.push({ target: value.slice(0, separator), binary: value.slice(separator + 1) });
        break;
      }
      default:
        throw new Error(`Unknown argument: ${argv[index]}`);
    }
  }
  if (clients.some((client) => !client)) throw new Error("--client requires a name");
  if (binary && mcpb.length) throw new Error("Choose --binary or --mcpb, not both");
  return {
    clients,
    binary,
    mcpb: mcpb.length ? mcpb : undefined,
    output: join(repositoryRoot, "target", "integrations"),
  };
}

const isEntryPoint = process.argv[1]
  && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));
if (isEntryPoint) {
  try {
    const outputs = buildIntegrationPackages(parseArgs(process.argv.slice(2)));
    process.stdout.write(`${outputs.map((path) => basename(path)).join("\n")}\n`);
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
