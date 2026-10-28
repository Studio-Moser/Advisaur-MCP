#!/usr/bin/env node
// Compares a base evaluated by its pre-rename evaluator (shelby-memory-eval) with a candidate
// evaluated by assistant-memory-eval (ADR-0012). The two evaluators name their suites and corpus
// differently, so the evaluator's own `compare` reports drift; this gate checks what a rename must
// not change: the same policy and cases, every base pass still passing, each case's ranking and
// contract output (beyond the renamed brief header), and no aggregate metric falling.
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

// What the rename itself changes in a case's output: the brief header names the product, which
// also moves the brief's size by a few bytes.
const RENAMED_HEADER = ["## Advisaur memory context", "## Shelby memory context"];
const SIZE_FIELDS = new Set(["estimated_tokens", "serialized_bytes"]);

function normalized(value) {
  if (typeof value === "string") return value.replaceAll(...RENAMED_HEADER);
  if (Array.isArray(value)) return value.map(normalized);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value)
        .filter(([key]) => !SIZE_FIELDS.has(key))
        .map(([key, item]) => [key, normalized(item)]),
    );
  }
  return value;
}

export function compareRenamed(base, candidate) {
  const failures = [];
  if (base.policy_version !== candidate.policy_version) {
    failures.push(`policy version changed from ${base.policy_version} to ${candidate.policy_version}`);
  }
  if (base.schema_version !== candidate.schema_version) {
    failures.push(`results schema changed from ${base.schema_version} to ${candidate.schema_version}`);
  }
  // Every input but the renamed contract corpus must be the same; that one keeps its source and
  // license (its name, revision and hash follow the rename).
  const corpus = (entry) => entry.source?.endsWith("Contract Corpus-v1.json");
  const inputs = (results) => (results.provenance ?? []).filter((entry) => !corpus(entry));
  if (JSON.stringify(inputs(base)) !== JSON.stringify(inputs(candidate))) {
    failures.push("the evaluation's inputs changed");
  }
  const corpusOf = (results) => (results.provenance ?? []).find(corpus);
  const [baseCorpus, candidateCorpus] = [corpusOf(base), corpusOf(candidate)];
  if (baseCorpus?.source !== candidateCorpus?.source || baseCorpus?.license !== candidateCorpus?.license) {
    failures.push("the contract corpus changed source or license");
  }
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
    if (!other) continue;
    if (entry.passed && !other.passed) failures.push(`case ${id} passed on the base and fails now`);
    // Beyond the rename's own header, each case must return what it did: the same ranking, the
    // same contract output.
    for (const field of ["ranked_ids", "relevant_ranks", "metrics", "output", "failures"]) {
      if (JSON.stringify(normalized(entry[field])) !== JSON.stringify(normalized(other[field]))) {
        failures.push(`case ${id} ${field} changed`);
      }
    }
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
    ? `Renamed evaluator gate passed: ${candidate.cases.length} cases, same outputs and rankings, no pass lost, no aggregate fell.\n`
    : `Renamed evaluator gate failed:\n${failures.map((failure) => `- ${failure}`).join("\n")}\n`;
  process.stdout.write(summary);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `\n${summary}`);
  process.exitCode = failures.length === 0 ? 0 : 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
