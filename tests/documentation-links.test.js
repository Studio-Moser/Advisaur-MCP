import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { checkDocuments, extractLinks, headingAnchors } from "../scripts/Check Documentation Links.mjs";

test("extracts local links while ignoring fenced code and web URLs", () => {
  const markdown = [
    "[local](docs/Guide.md#hello-world)",
    "[web](https://example.com/missing)",
    "```md",
    "[example](missing.md)",
    "```",
  ].join("\n");
  assert.deepEqual(extractLinks(markdown), [{ line: 1, target: "docs/Guide.md#hello-world" }]);
});

test("builds GitHub-style heading anchors including duplicates", () => {
  assert.deepEqual(
    [...headingAnchors("# Hello, World!\n## Hello, World!\n<a id=\"exact\"></a>\n")],
    ["hello-world", "hello-world-1", "exact"],
  );
});

test("reports the source line for missing files and anchors", () => {
  const root = mkdtempSync(join(tmpdir(), "advisaur-mcp-doc-links-"));
  mkdirSync(join(root, "docs"));
  writeFileSync(join(root, "README.md"), [
    "[good](docs/Guide.md#real-heading)",
    "[missing file](docs/Missing.md)",
    "[missing anchor](docs/Guide.md#not-there)",
  ].join("\n"));
  writeFileSync(join(root, "docs", "Guide.md"), "# Real heading\n");

  assert.deepEqual(checkDocuments(root, ["README.md", "docs/Guide.md"]), [
    "README.md:2: missing target docs/Missing.md",
    "README.md:3: missing anchor #not-there in docs/Guide.md",
  ]);
});
