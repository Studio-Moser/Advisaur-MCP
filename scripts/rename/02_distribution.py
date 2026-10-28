#!/usr/bin/env python3
"""ADR-0012 rename, step 4 (Advisaur-MCP#78): the npm packages, registry entry, client
integrations, plugins and skills take the brand-visible names (D4).

The npm package, platform packages, launcher, config keys and integration manifests are
advisaur-mcp; the MCP Registry entry io.github.Studio-Moser/advisaur-mcp; the skills
advisaur-forage and advisaur-onboard; repository links Studio-Moser/Advisaur-MCP. The Claude Code
marketplace and plugin are `advisaur` (D4's one exception), set by hand afterwards. The old data
folder (`.shelbymcp`) and the legacy key constant keep their names; docs follow in the next step.
"""
import pathlib, re, subprocess

ROOT = pathlib.Path(__file__).resolve().parents[2]

MOVES = [
    ("bin/shelbymcp.js", "bin/advisaur-mcp.js"),
    ("integrations/codex/shelbymcp", "integrations/codex/advisaur-mcp"),
    ("skills/shelby-forage", "skills/advisaur-forage"),
    ("skills/shelby-onboard", "skills/advisaur-onboard"),
]

FILES = re.compile(r"""(?x)^(
    package\.json | server\.json | bin/ | npm/ | integrations/ | skills/ | \.claude-plugin/
  | scripts/[^/]+\.mjs$ | tests/[^/]+\.js$ | \.github/workflows/ | assets/
  | crates/assistant-mcp/src/commands\.rs$ | crates/assistant-mcp/src/prompts\.rs$
)""")

RULES = [
    (r"github\.com/Studio-Moser/shelbymcp", "github.com/Studio-Moser/Advisaur-MCP"),
    (r"github:Studio-Moser/shelbymcp", "github:Studio-Moser/Advisaur-MCP"),
    (r"io\.github\.Studio-Moser/shelbymcp", "io.github.Studio-Moser/advisaur-mcp"),
    (r"\bshelby-(forage|onboard)\b", r"advisaur-\1"),
    (r"\bShelby (Forage|Onboard)\b", r"Advisaur \1"),
    (r"\bshelby-mcp-", "advisaur-mcp-"),  # release assets
    (r"(?<![.\w])shelbymcp(?![\w])", "advisaur-mcp"),
    (r"\bShelbyMCP\b", "Advisaur MCP"),
    (r"\bShelby\b", "Advisaur"),
]


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, check=True).stdout


def main():
    for old, new in MOVES:
        if (ROOT / old).exists():
            git("mv", old, new)
    changed = 0
    for f in git("ls-files").split("\n"):
        if not f or not FILES.match(f):
            continue
        p = ROOT / f
        try:
            src = p.read_text()
        except UnicodeDecodeError:
            continue
        new = src
        for pattern, replacement in RULES:
            new = re.sub(pattern, replacement, new)
        if new != src:
            p.write_text(new)
            changed += 1
    print(f"{changed} files")


if __name__ == "__main__":
    main()
