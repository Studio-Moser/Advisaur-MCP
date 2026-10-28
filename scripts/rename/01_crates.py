#!/usr/bin/env python3
"""ADR-0012 rename, step 1 (Advisaur-MCP#78): crates, folders, imports and the binary.

Crates drop the brand for the neutral codename (D3): assistant-memory, assistant-integrations,
assistant-mcp, assistant-memory-eval. The server binary is brand-visible (D4): advisaur-mcp.
`ShelbyServer` is `MemoryServer`. A `shelby-mcp` that names the crate (a cargo package, its
folder, a crate path) becomes assistant-mcp; one a person runs or reads (CLI usage, executable
paths, log lines, the version line) becomes advisaur-mcp. History (CHANGELOG, docs/superpowers)
and shared fixture data keep their names.
"""
import pathlib, re, subprocess

ROOT = pathlib.Path(__file__).resolve().parents[2]

CRATES = {
    "shelby-memory-eval": "assistant-memory-eval",
    "shelby-memory": "assistant-memory",
    "shelby-integrations": "assistant-integrations",
    "shelby-mcp": "assistant-mcp",
}

SKIP = re.compile(r"^(CHANGELOG\.md|docs/superpowers/|Cargo\.lock$|package-lock\.json$|scripts/rename/|tests/fixtures/)")

# A `shelby-mcp` that names the crate, not the binary.
CRATE_CONTEXT = [
    (r"-p shelby-mcp\b", "-p assistant-mcp"),
    (r"\bshelby-mcp::", "assistant_mcp::"),
    (r"### `shelby-mcp`", "### `assistant-mcp`"),
    (r"- `shelby-mcp` exposes the MCP service", "- `assistant-mcp` exposes the MCP service"),
    (r"^(\s*)shelby-mcp/(\s+MCP library)", r"\1assistant-mcp/\2"),
    (r"^(\s+)shelby-mcp$", r"\1assistant-mcp"),  # the architecture diagram's crate box
    (r"^# shelby-mcp$", "# assistant-mcp"),
]


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, check=True).stdout


def rewrite(path, text):
    for old, new in CRATES.items():
        text = text.replace(f"crates/{old}", f"crates/{new}")
        text = text.replace(f"../{old}", f"../{new}")
    if path.endswith("Cargo.toml"):
        text = re.sub(r'(?m)^\[\[bin\]\]\nname = "shelby-mcp"', '[[bin]]\nname = "advisaur-mcp"', text)
        for old, new in CRATES.items():
            text = re.sub(rf'(?m)^name = "{old}"', f'name = "{new}"', text)
            text = re.sub(rf"(?m)^{old} = \{{", f"{new} = {{", text)
    if path.endswith(".rs") or path.endswith(".md"):
        for old, new in CRATES.items():
            text = re.sub(rf"\b{old.replace('-', '_')}\b", new.replace("-", "_"), text)
        text = re.sub(r"\bShelbyServer\b", "MemoryServer", text)
    for old, new in CRATE_CONTEXT:
        text = re.sub(old, new, text, flags=re.M)
    for old, new in CRATES.items():
        text = re.sub(rf"-p {old}\b", f"-p {new}", text)
    text = text.replace("CARGO_BIN_EXE_shelby-memory-eval", "CARGO_BIN_EXE_assistant-memory-eval")
    text = text.replace("CARGO_BIN_EXE_shelby-mcp", "CARGO_BIN_EXE_advisaur-mcp")
    # Everything else named shelby-mcp is the binary.
    text = re.sub(r"(?<![\w-])shelby-mcp(?![\w-])", "advisaur-mcp", text)
    text = re.sub(r"(?<![\w-])shelby-memory-eval(?![\w-])", "assistant-memory-eval", text)
    text = re.sub(r"(?<![\w-])shelby-memory(?![\w-])", "assistant-memory", text)
    text = re.sub(r"(?<![\w-])shelby-integrations(?![\w-])", "assistant-integrations", text)
    return text


def main():
    for old, new in CRATES.items():
        if (ROOT / "crates" / old).exists():
            git("mv", f"crates/{old}", f"crates/{new}")
    changed = 0
    for f in git("ls-files").split("\n"):
        if not f or SKIP.match(f):
            continue
        p = ROOT / f
        try:
            src = p.read_text()
        except (UnicodeDecodeError, IsADirectoryError):
            continue
        new = rewrite(f, src)
        if new != src:
            p.write_text(new)
            changed += 1
    print(f"{changed} files")


if __name__ == "__main__":
    main()
