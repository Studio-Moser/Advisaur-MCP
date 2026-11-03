#!/usr/bin/env node
// Writes tests/fixtures/Candidate Pool Corpus-v1.json, the input of
// `assistant-memory-eval pool-benchmark` (Advisaur-Docs#548). The output is deterministic:
// a seeded PRNG drives every random choice, so rerunning this script reproduces the fixture
// byte for byte. `docs/Candidate Pool Benchmark-v1.md` explains how each group is built.
//
//   node "scripts/Generate Candidate Pool Corpus.mjs"

import { writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const output = join(repositoryRoot, "tests", "fixtures", "Candidate Pool Corpus-v1.json");

// Vector layout: 0-5 small-project topics, 6-9 small-project aspects, 10 a null axis that
// is orthogonal to every query, 11-13 stress axes, 14-21 workspace topics, 22-25 workspace
// aspects.
const DIM = 26;
const NULL = 10;
const KESTREL = 11;
const HERON = 12;
const OSPREY = 13;
const LARGE_TOPIC = 14;
const LARGE_ASPECT = 22;

let seed = 548;
function random() {
  // mulberry32
  seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}
function gauss(sigma) {
  const u = 1 - random();
  return sigma * Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * random());
}
function pick(items) {
  return items[Math.floor(random() * items.length)];
}
function zeros() {
  return new Array(DIM).fill(0);
}
function unit(axis) {
  const v = zeros();
  v[axis] = 1;
  return v;
}
// A vector whose cosine with the query `unit(axis)` is exactly `cos`.
function mix(cos, axis) {
  const v = zeros();
  v[axis] = cos;
  v[NULL] = Math.sqrt(Math.max(0, 1 - cos * cos));
  return v;
}
function round(v) {
  return v.map((x) => {
    const r = Math.round(x * 1e4) / 1e4;
    return r === 0 ? 0 : r;
  });
}

const thoughts = [];
let minute = 0;
function add(id, content, type, project, embedding) {
  minute += 1;
  const day = String(1 + Math.floor(minute / 1440)).padStart(2, "0");
  const hour = String(Math.floor(minute / 60) % 24).padStart(2, "0");
  const min = String(minute % 60).padStart(2, "0");
  thoughts.push({
    id,
    content,
    type,
    project_identifier: project,
    embedding: round(embedding),
    created_at: `2026-10-${day}T${hour}:${min}:00Z`,
  });
}

// --- Small-project representative notes: 6 topics x 4 aspects x 2 notes. ---
const SMALL = [
  [
    ["SQLite WAL mode lets the MCP server read while the app writes to the memory store.", "Keep WAL mode on; a rollback journal blocked readers during long capture batches."],
    ["Backups copy the database with the SQLite online backup API after a WAL checkpoint.", "Run a checkpoint before each nightly backup so the copied file is complete."],
    ["Schema migrations run inside one transaction and bump user_version last.", "A failed migration rolls back and leaves the schema version untouched."],
    ["Vacuum the database monthly; deleted thoughts leave free pages behind.", "Incremental vacuum reclaims pages without locking the store for long."],
  ],
  [
    ["Full-text search uses FTS5 with the porter tokenizer and BM25 rank.", "Prefix matching in the FTS query finds plurals like decisions and decision."],
    ["Hybrid search fuses keyword and vector results with reciprocal rank fusion.", "Reciprocal rank fusion uses K of sixty so neither source dominates the merged ranking."],
    ["Callers supply embeddings; the server never runs an embedding model itself.", "Embedding vectors are stored as float32 little-endian blobs beside each thought."],
    ["Vector search drops candidates below a cosine similarity threshold of 0.3.", "Raising the similarity threshold hid paraphrases, so it stays at 0.3."],
  ],
  [
    ["The HTTP transport uses OAuth with PKCE for remote clients.", "OAuth tokens expire after an hour and refresh silently."],
    ["Local clients authenticate with a bearer token stored in the hook token file.", "Rotate the bearer token whenever the hook token file is shared by mistake."],
    ["Provider credentials never reach the server; the Mac app keeps them in the Keychain.", "Keychain items fall back to local storage when the access group entitlement is missing."],
    ["Read tools require the memory read scope; capture requires the write scope.", "A token without the write scope gets an actionable error on capture."],
  ],
  [
    ["The npm launcher downloads the native binary for the current platform.", "Platform packages publish under optional dependencies so npm installs only one binary."],
    ["Release builds are signed and notarized before the DMG is published.", "Notarization failures usually mean a nested binary lacks the hardened runtime."],
    ["The changelog Unreleased section becomes the release notes in the appcast.", "Write changelog entries for users, not for reviewers."],
    ["Bump the short version and the build number together for every release.", "The release tag comes from the short version string."],
  ],
  [
    ["Projects are identified by immutable UUIDs, not by their slugs.", "Renaming a project keeps its UUID and records the old slug as an alias."],
    ["Aliases let old project identifiers resolve after a rename.", "An alias that points at two projects is ambiguous and fails closed."],
    ["Project-scoped search never returns another project's private thoughts.", "Shared thoughts appear in other projects only when include_shared is set."],
    ["Project detection reads the git remote before falling back to the folder name.", "Detection prefers the git remote because folder names collide across machines."],
  ],
  [
    ["Briefs stay inside the caller's token budget and drop low-priority items first.", "The brief token estimate counts four characters per token."],
    ["Untrusted memory text is fenced in briefs so it cannot issue instructions.", "External captures default to the untrusted trust level until confirmed."],
    ["Superseded decisions are excluded from briefs once consolidated.", "Briefs prefer the newest confirmed decision over a stale duplicate."],
    ["A brief opens with active decisions, then recent work, then references.", "Brief sections are omitted when they have no items."],
  ],
];
const TYPES = ["decision", "reference", "note", "observation"];
const SMALL_OTHER = [
  [0, 1, "The other project also runs a checkpoint before its backup job."],
  [1, 1, "Rank fusion in the other project weights keyword results higher."],
  [2, 1, "The other project stores its bearer token in an environment variable."],
  [4, 1, "The other project never uses an alias for its repository."],
  [1, 3, "The other project tuned its similarity threshold down to 0.2."],
  [5, 1, "The other project fences untrusted web content in its prompts."],
];
function smallVector(topic, aspect) {
  const v = zeros();
  v[topic] = 1;
  v[6 + aspect] = 0.6;
  for (let i = 0; i < 10; i += 1) v[i] += gauss(0.15);
  return v;
}
SMALL.forEach((aspects, t) =>
  aspects.forEach((pair, a) =>
    pair.forEach((text, n) =>
      add(`rep-t${t}-a${a}-n${n + 1}`, text, TYPES[(a + n) % 4], "advisaur", smallVector(t, a)),
    ),
  ),
);
SMALL_OTHER.forEach(([t, a, text], i) =>
  add(`rep-other-${String(i + 1).padStart(2, "0")}`, text, "note", "other-project", smallVector(t, a)),
);

// --- Workspace representative notes: 8 topics x (4 aspects x 4 cell notes + 24 general). ---
// Every note names its topic word and two other topic words, so a one-word query matches
// well over 100 notes. Cell notes 1-3 use the aspect word; cell note 4 paraphrases it.
const LARGE = [
  ["sqlite", [["wal", "WAL mode", "the write-ahead log"], ["backup", "the nightly backup", "copying the store each night"], ["migration", "a schema migration", "a schema upgrade"], ["vacuum", "the monthly vacuum", "reclaiming free pages"]]],
  ["search", [["fusion", "rank fusion", "merging keyword and vector lists"], ["threshold", "the similarity threshold", "the cosine cutoff"], ["prefix", "prefix matching", "matching word stems"], ["latency", "query latency", "slow queries"]]],
  ["token", [["refresh", "a silent refresh", "renewing credentials"], ["scope", "the write scope", "missing permissions"], ["keychain", "the Keychain item", "the credential store"], ["rotation", "bearer rotation", "replacing a leaked secret"]]],
  ["release", [["notarize", "notarize the app", "the Apple signing check"], ["changelog", "the changelog", "the user-facing notes"], ["appcast", "the appcast feed", "the update feed"], ["dmg", "the DMG", "the disk image"]]],
  ["project", [["alias", "a slug alias", "an old name that still resolves"], ["rename", "a rename", "changing the slug"], ["detection", "remote detection", "guessing from the git remote"], ["uuid", "the immutable UUID", "the stable identifier"]]],
  ["brief", [["budget", "the size budget", "the size limit"], ["fence", "the trust fence", "quoting untrusted text"], ["stale", "stale decisions", "superseded entries"], ["sections", "the sections", "the layout"]]],
  ["sync", [["conflict", "a conflict", "two edits to the same thought"], ["cloudkit", "CloudKit records", "the iCloud container"], ["offline", "offline edits", "edits without a network"], ["merge", "the merge step", "combining changes"]]],
  ["import", [["swift", "the Swift importer", "the Mac app's data"], ["transcript", "chat transcripts", "saved conversations"], ["dedupe", "dedupe passes", "dropping repeated captures"], ["watcher", "the folder watcher", "file system events"]]],
];
const LEADS = ["Decision:", "Note:", "Observed:", "Follow-up:", "Reminder:"];
const TOPIC_PHRASES = ["the {w} layer", "{w} handling", "our {w} code", "the {w} path", "{w} behaviour"];
const GENERIC = [
  "needs a regression test",
  "was slow on the older Mac",
  "came up during code review",
  "should stay boring and observable",
  "broke once on a fresh install",
  "is described in the architecture notes",
  "matters in the first week of use",
  "has a known edge case with empty input",
  "",
  "",
];
function otherTopics(t) {
  const first = (t + 1 + Math.floor(random() * 7)) % 8;
  let second = first;
  while (second === first || second === t) second = Math.floor(random() * 8);
  return [first, second];
}
function largeNote(t, aspectPhrase) {
  const [word] = LARGE[t];
  const [m1, m2] = otherTopics(t);
  const parts = [pick(LEADS), pick(TOPIC_PHRASES).replace("{w}", word)];
  if (aspectPhrase) parts.push(`and ${aspectPhrase}`);
  const generic = pick(GENERIC);
  if (generic) parts.push(generic);
  return { text: `${parts.join(" ")}; related to ${LARGE[m1][0]} and ${LARGE[m2][0]} work.`, mentions: [m1, m2] };
}
function largeVector(t, aspect, aspectWeight, mentions) {
  const v = zeros();
  v[LARGE_TOPIC + t] = 1;
  v[LARGE_ASPECT + aspect] = aspectWeight;
  for (const m of mentions) v[LARGE_TOPIC + m] += 0.35;
  for (let i = LARGE_TOPIC; i < LARGE_ASPECT + 4; i += 1) v[i] += gauss(0.15);
  return v;
}
function largeProject(project, prefix, cellNotes, generalNotes) {
  LARGE.forEach(([, aspects], t) => {
    aspects.forEach(([, phrase, paraphrase], a) => {
      for (let n = 0; n < cellNotes; n += 1) {
        const note = largeNote(t, n === 3 ? paraphrase : phrase);
        add(`${prefix}-t${t}-a${a}-n${n + 1}`, note.text, pick(TYPES), project, largeVector(t, a, 0.6, note.mentions));
      }
    });
    for (let n = 0; n < generalNotes; n += 1) {
      const note = largeNote(t, null);
      add(`${prefix}-t${t}-g${String(n + 1).padStart(2, "0")}`, note.text, pick(TYPES), project, largeVector(t, Math.floor(random() * 4), 0.3, note.mentions));
    }
  });
}
largeProject("workspace", "ws", 4, 24);
largeProject("sideline", "sl", 2, 4);

// --- Kestrel stress population. ---
// Lexical-only notes contain "kestrel" and point along the null axis; vector-only notes omit
// the word. Every note has the same token count, so BM25 orders them by term frequency, and
// each tail target's frequency sits between two tiers. The vector order is the same tier list
// with strictly falling cosines, so each target holds the same rank in both sources.
const TIERS = [[24, 8], ["t025", 7], [34, 6], ["t060", 5], [89, 4], ["t150", 3], [10, 2]];
const vectorOrder = [];
let vectorCount = 0;
for (const [tier] of TIERS) {
  if (typeof tier === "string") {
    vectorOrder.push(`kestrel-tail-${tier.slice(1)}`);
  } else {
    for (let i = 0; i < tier; i += 1) {
      vectorCount += 1;
      vectorOrder.push(`kestrel-vector-${String(vectorCount).padStart(3, "0")}`);
    }
  }
}
const cosine = new Map(vectorOrder.map((id, i) => [id, Math.round((0.98 - 0.003 * i) * 1e4) / 1e4]));
const body = (word, tf) => [...new Array(tf).fill(word), ...new Array(12 - tf).fill("pad")].join(" ");
let lexicalCount = 0;
for (const [tier, tf] of TIERS) {
  if (typeof tier === "string") {
    const id = `kestrel-tail-${tier.slice(1)}`;
    add(id, `stress tail ${tier}: ${body("kestrel", tf)}`, "note", "stress-lab", mix(cosine.get(id), KESTREL));
  } else {
    for (let i = 0; i < tier; i += 1) {
      lexicalCount += 1;
      const n = String(lexicalCount).padStart(3, "0");
      add(`kestrel-lexical-${n}`, `stress lexical l${n}: ${body("kestrel", tf)}`, "note", "stress-lab", unit(NULL));
    }
  }
}
for (let i = 1; i <= vectorCount; i += 1) {
  const id = `kestrel-vector-${String(i).padStart(3, "0")}`;
  add(id, `stress vector v${String(i).padStart(3, "0")}: ${body("pad", 0)}`, "note", "stress-lab", mix(cosine.get(id), KESTREL));
}

// --- Heron: each relevant note leads one source and is absent from the other. ---
[6, 3, 2].forEach((tf, i) =>
  add(`heron-lexical-${i + 1}`, `stress heron h${i + 1}: ${body("heron", tf)}`, "note", "stress-lab", unit(NULL)),
);
[0.95, 0.8, 0.7].forEach((cos, i) =>
  add(`heron-vector-${i + 1}`, `stress neighbor h${i + 1}: ${body("pad", 0)}`, "note", "stress-lab", mix(cos, HERON)),
);

// --- Osprey: strong distractors fail the project or type filter; the two relevant decisions
// are weak in both sources. ---
for (let i = 1; i <= 40; i += 1) {
  add(`osprey-other-${String(i).padStart(3, "0")}`, `stress osprey o: ${body("osprey", 6)}`, "decision", "other-lab", mix(0.95 - 0.002 * i, OSPREY));
}
for (let i = 1; i <= 30; i += 1) {
  add(`osprey-note-${String(i).padStart(3, "0")}`, `stress osprey n: ${body("osprey", 6)}`, "note", "stress-lab", mix(0.94 - 0.002 * i, OSPREY));
}
for (let i = 1; i <= 2; i += 1) {
  add(`osprey-decision-${i}`, `stress osprey d: ${body("osprey", i)}`, "decision", "stress-lab", mix(0.6 - 0.05 * i, OSPREY));
}

// --- Cases. ---
const cases = [];
function addCase(id, cls, category, query, embedding, scope, relevant, type) {
  const c = { id, class: cls, category, query, embedding: round(embedding) };
  if (scope) c.project_identifier = scope;
  else c.all_projects = true;
  c.relevant_ids = relevant;
  if (type) c.type = type;
  cases.push(c);
}
function smallQuery(t, a) {
  const v = zeros();
  v[t] = 1;
  v[6 + a] = 0.6;
  return v;
}
function largeQuery(t, a) {
  const v = zeros();
  v[LARGE_TOPIC + t] = 1;
  v[LARGE_ASPECT + a] = 0.6;
  return v;
}
const small = "representative-small";
addCase("rep-checkpoint-backup", small, "lexical-and-vector-agree", "checkpoint backup", smallQuery(0, 1), "advisaur", ["rep-t0-a1-n1", "rep-t0-a1-n2"]);
addCase("rep-rank-fusion", small, "lexical-and-vector-agree", "rank fusion", smallQuery(1, 1), "advisaur", ["rep-t1-a1-n1", "rep-t1-a1-n2"]);
addCase("rep-similarity-threshold", small, "lexical-and-vector-agree", "similarity threshold", smallQuery(1, 3), "advisaur", ["rep-t1-a3-n1", "rep-t1-a3-n2"]);
addCase("rep-bearer-token", small, "shared-term-competition", "bearer token", smallQuery(2, 1), "advisaur", ["rep-t2-a1-n1", "rep-t2-a1-n2"]);
addCase("rep-project-alias", small, "shared-term-competition", "alias", smallQuery(4, 1), "advisaur", ["rep-t4-a1-n1", "rep-t4-a1-n2"]);
addCase("rep-untrusted-fence", small, "lexical-vector-partial", "fenced untrusted", smallQuery(5, 1), "advisaur", ["rep-t5-a1-n1", "rep-t5-a1-n2"]);

// Even topics ask with the topic word alone (broad lexical match, intent in the vector);
// odd topics add the aspect word (focused lexical match).
const large = "representative-large";
const cell = (prefix, t, a, n) => Array.from({ length: n }, (_, i) => `${prefix}-t${t}-a${a}-n${i + 1}`);
LARGE.forEach(([word, aspects], t) => {
  const a = t % 4;
  const broad = t % 2 === 0;
  addCase(
    `ws-${word}-${aspects[a][0]}`,
    large,
    broad ? "broad-topic-query" : "focused-topic-aspect-query",
    broad ? word : `${word} ${aspects[a][0]}`,
    largeQuery(t, a),
    "workspace",
    cell("ws", t, a, 4),
  );
});
addCase("all-sqlite-backup", large, "all-projects-focused", "sqlite backup", largeQuery(0, 1), null, [...cell("ws", 0, 1, 4), ...cell("sl", 0, 1, 2)]);
addCase("all-import-transcript", large, "all-projects-broad", "import", largeQuery(7, 1), null, [...cell("ws", 7, 1, 4), ...cell("sl", 7, 1, 2)]);

const stress = "stress";
addCase("stress-tail-025", stress, "relevant-tail-candidate", "kestrel", unit(KESTREL), "stress-lab", ["kestrel-tail-025"]);
addCase("stress-tail-060", stress, "relevant-tail-candidate", "kestrel", unit(KESTREL), "stress-lab", ["kestrel-tail-060"]);
addCase("stress-tail-150", stress, "relevant-tail-candidate", "kestrel", unit(KESTREL), "stress-lab", ["kestrel-tail-150"]);
addCase("stress-lexical-vector-disagreement", stress, "lexical-vector-disagreement", "heron", unit(HERON), "stress-lab", ["heron-lexical-1", "heron-vector-1"]);
addCase("stress-project-type-filter", stress, "project-type-filter", "osprey", unit(OSPREY), "stress-lab", ["osprey-decision-1", "osprey-decision-2"], "decision");

const fixture = {
  schema_version: 1,
  suite_version: "candidate-pool-v1",
  embedding_dimensions: DIM,
  projects: ["advisaur", "other-project", "workspace", "sideline", "stress-lab", "other-lab"],
};
const lines = ["{"];
for (const [key, value] of Object.entries(fixture)) lines.push(`  "${key}": ${JSON.stringify(value)},`);
lines.push('  "thoughts": [', thoughts.map((t) => `    ${JSON.stringify(t)}`).join(",\n"), "  ],");
lines.push('  "cases": [', cases.map((c) => `    ${JSON.stringify(c)}`).join(",\n"), "  ]");
lines.push("}");
writeFileSync(output, `${lines.join("\n")}\n`);
console.log(`wrote ${thoughts.length} thoughts and ${cases.length} cases to ${output}`);
