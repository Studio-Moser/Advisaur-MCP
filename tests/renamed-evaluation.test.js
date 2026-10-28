import assert from "node:assert/strict";
import test from "node:test";
import { compareRenamed } from "../scripts/Compare Renamed Evaluation.mjs";

const results = (cases, recall) => ({
  cases: cases.map(([id, passed]) => ({ id, passed })),
  aggregates: { overall: { recall_at_5: recall } },
});

test("a rename that keeps every case and metric passes", () => {
  assert.deepEqual(compareRenamed(results([["a", true], ["b", false]], 0.5), results([["a", true], ["b", true]], 0.5)), []);
});

test("a lost pass, a missing case or a fallen metric fails", () => {
  const failures = compareRenamed(results([["a", true], ["b", true]], 0.5), results([["a", false]], 0.4));
  assert.equal(failures.length, 3);
  assert.match(failures.join("\n"), /case a passed on the base/);
  assert.match(failures.join("\n"), /case b is missing/);
  assert.match(failures.join("\n"), /recall_at_5 fell/);
});

test("a changed contract output or policy fails, the renamed header doesn't", () => {
  const with_ = (output, policy = 1) => ({
    policy_version: policy,
    schema_version: 1,
    cases: [{ id: "a", passed: true, output, estimated_tokens: output.length }],
    aggregates: {},
  });
  assert.deepEqual(compareRenamed(with_("## Shelby memory context\nX"), with_("## Advisaur memory context\nX")), []);
  assert.match(compareRenamed(with_("## Shelby memory context\nX"), with_("## Advisaur memory context\nY")).join(), /case a output changed/);
  assert.match(compareRenamed(with_("X"), with_("X", 2)).join(), /policy version changed/);
});
