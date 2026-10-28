#!/usr/bin/env node
// Compares a base evaluated by its pre-rename evaluator (shelby-memory-eval) with a candidate
// evaluated by assistant-memory-eval (ADR-0012). The two evaluators name their suites and corpus
// differently, so the evaluator's own `compare` reports drift; this gate checks what a rename must
// not change: the same cases, every base pass still passing, and no aggregate metric falling.
//
//   node "scripts/Compare Renamed Evaluation.mjs" --base base.json --candidate candidate.json

import { appendFileSync, readFileSync } from "node:fs";
import process from "node:process";
import { pathToFileURL } from "node:url";

function argument(name) {
  const index = process.argv.indexOf(name);
  if (index < 0 || !process.argv[index + 1]) {
    throw new Error(`missing ${name}`);
  }
  return process.argv[index + 1];
}

export function compareRenamed(base, candidate) {
  const failures = [];
  const byId = (results) => new Map(results.cases.map((entry) => [entry.id, entry]));
  const baseCases = byId(base);
  const candidateCases = byId(candidate);
  for (const id of baseCases.keys()) {
    if (!candidateCases.has(id)) failures.push(`case ${id} is missing from the candidate`);
  }
  for (const id of candidateCases.keys()) {
    if (!baseCases.has(id)) failures.push(`case ${id} is new in the candidate`);
  }
  for (const [id, entry] of baseCases) {
    const other = candidateCases.get(id);
    if (other && entry.passed && !other.passed) failures.push(`case ${id} passed on the base and fails now`);
  }
  for (const [category, metrics] of Object.entries(base.aggregates ?? {})) {
    for (const [metric, value] of Object.entries(metrics)) {
      const now = candidate.aggregates?.[category]?.[metric];
      if (typeof now !== "number" || now < value) {
        failures.push(`${category} ${metric} fell from ${value} to ${now}`);
      }
    }
  }
  return failures;
}

function main() {
  const base = JSON.parse(readFileSync(argument("--base"), "utf8"));
  const candidate = JSON.parse(readFileSync(argument("--candidate"), "utf8"));
  const failures = compareRenamed(base, candidate);
  const summary = failures.length === 0
    ? `Renamed evaluator gate passed: ${candidate.cases.length} cases, no pass lost, no aggregate fell.\n`
    : `Renamed evaluator gate failed:\n${failures.map((failure) => `- ${failure}`).join("\n")}\n`;
  process.stdout.write(summary);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `\n${summary}`);
  process.exitCode = failures.length === 0 ? 0 : 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
