#!/usr/bin/env python3
"""ADR-0012 rename, step 5 (Advisaur-MCP#78): the current docs follow the renamed server.

History stays as written: the CHANGELOG and docs/superpowers. The archived Shelby-App and the
Swift-import contract (`shelby-swift*`, `shelby_swift_import_*`) keep their names.
"""
import pathlib, re, subprocess

ROOT = pathlib.Path(__file__).resolve().parents[2]

DOCS = re.compile(r"""(?x)^(
    README\.md | AGENTS\.md | CLAUDE\.md | SECURITY\.md | CONTRIBUTING\.md | CODE_OF_CONDUCT\.md
  | docs/[^/]+\.md | crates/[^/]+/README\.md | \.github/ISSUE_TEMPLATE/.+\.md
)$""")

KEEP = re.compile(r"Shelby-App|Rust Shelby|shelby-swift[\w-]*|shelby_swift_import_\w*|shelby-project-v1:?")

RULES = [
    (r'^\s*<img src="docs/assets/shelby-mcp-header\.png"[^>]*/>\n', ""),
    (r"github\.com/Studio-Moser/shelbymcp(\.git)?", r"github.com/Studio-Moser/Advisaur-MCP\1"),
    (r"\bShelby-(MCP|MacOS|Docs)\b", r"Advisaur-\1"),
    (r"io\.github\.Studio-Moser/shelbymcp", "io.github.Studio-Moser/advisaur-mcp"),
    (r"~/\.shelbymcp\b", "~/.studiomoser-assistant-mcp"),
    (r"(?<![\w~])\.shelbymcp\b", ".studiomoser-assistant-mcp"),
    (r"\bSHELBY_(DB_PATH|TRANSPORT|API_KEY)\b", r"ADVISAUR_\1"),
    (r"\bSHELBY_([A-Z_]+)\b", r"ASSISTANT_\1"),
    (r"\bshelby-(forage|onboard)\b", r"advisaur-\1"),
    (r"\bShelby (Forage|Onboard)\b", r"Advisaur \1"),
    (r"(?<![\w.])shelbymcp(?![\w])", "advisaur-mcp"),
    (r"\bShelbyMCP\b", "Advisaur MCP"),
    (r"\bShelby\b", "Advisaur"),
]


def rewrite(text):
    kept = []

    def mask(m):
        kept.append(m.group(0))
        return f"{len(kept) - 1}"

    text = KEEP.sub(mask, text)
    for pattern, replacement in RULES:
        text = re.sub(pattern, replacement, text, flags=re.M)
    return re.sub("(\\d+)", lambda m: kept[int(m.group(1))], text)


def main():
    files = subprocess.run(["git", "ls-files"], cwd=ROOT, capture_output=True, text=True, check=True).stdout
    changed = 0
    for f in files.split("\n"):
        if f and DOCS.match(f):
            p = ROOT / f
            src = p.read_text()
            new = rewrite(src)
            if new != src:
                p.write_text(new)
                changed += 1
    print(f"{changed} files")


if __name__ == "__main__":
    main()
