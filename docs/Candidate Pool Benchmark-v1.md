# Hybrid candidate-pool benchmark (Advisaur-Docs#548)

Recorded result of `assistant-memory-eval pool-benchmark` at code `b3836ab`. The generated report follows the findings unchanged. `Candidate Pool Benchmark-v1.json` beside it holds every observation: the page IDs for every pool, limit, case and offset, plus each relevant thought's rank in the whole fused pool. Runtimes come from one release build on one Mac and are indicative only. Reproduce with:

```bash
node "scripts/Generate Candidate Pool Corpus.mjs"   # rewrites the fixture; output is byte-identical
cargo run --release -p assistant-memory-eval -- pool-benchmark --output target/pool-benchmark --code-sha "$(git rev-parse --short HEAD)"
```

## Findings

1. **Small-project representative cases: no change, by construction.** All six score 1.0 under every pool, but their filtered fused pools hold only 11–13 candidates. That is below the smallest production pool of 15 per source, so no pool size could change them. They answer nothing about pool size and are kept as a separate subgroup.
2. **Large-corpus representative cases: larger pools were neutral or worse.** Ten cases, eight in a 320-note project and two across all projects. At the complete pool their fused match sets hold 70–161 candidates (mean 108.8).
   - At limit 5, every pool from 30 up gives −0.083 Recall@5 and −0.062 NDCG@5.
   - At limit 10, pool 30 equals the current pool; pools of 100 or more give −0.117 Recall@10 and −0.055 NDCG@10.
   - The five focused queries (topic and aspect word) are identical under every pool.
   - The five broad queries (topic word only, intent carried by the vector) lose. Their relevant notes move down the fused ranking as the pool grows, for example `ws-project-alias` n1 goes from 4 to 8 to 19.
   - Inference, not measured separately: in a larger pool, more non-relevant notes collect contributions from both sources and outrank relevant notes that score well in only one. This text is generated from templates, so BM25 order here is unrelated to relevance. That may overstate the harm against real notes.
3. **The stress controls detect truncation.**
   - A target at rank 25 of both sources is missing at limit 5 under the current pool and ranked 1st with 30.
   - A target at rank 60 is missing under the current pool at both limits and ranked 2nd with 100.
   - The rank-150 target is outside the fused pool at 100 and is admitted at 200, at fused rank 90, still below the first page.
   - Stress means: +0.40 Recall@5 at limit 5 and +0.20 Recall@10 at limit 10 for pools of 100 or more. Nothing gains above 100.
4. **Filters run before pooling.** The project/type control keeps its two weak decisions at ranks 1–2 under every pool, and the lexical/vector disagreement control does not change.
5. **`total_count` reports the fused pool, not the match set.** For the kestrel query, 317 thoughts match, but `total_count` is 30 at limit 5 and 59 at limit 10. Pages beyond the fused pool come back empty with `has_more: false`. Whether that should change is the separate API decision the spec defers.
6. **Cost.** Against the current pool, median handler time for the stress cases is about 1.6–1.7× at 100 per source and about 2.2–2.3× at 200. The large-corpus cases are about 1.3–1.5× at 100 (see the tables).
7. **Parity.**
   - Within this branch, the override path given the production pool matched the default path in 126/126 calls. This checks the override plumbing only, because both paths run this branch's handler.
   - Two runs gave the same deterministic digest.
   - Parity with main is not automated. It was verified once by hand and once independently in review. The hand check extracted main `2d809e4` (`git archive`), copied in this evaluator with `search_thoughts_with_hybrid_pool` replaced by main's `search_thoughts_tool`, and ran it on this fixture. All 126 `current` observations matched this run, excluding fused ranks, which that replay cannot compute at the current pool.

None of this proposes a change. On this evidence a larger default pool is not justified: it helps only the synthetic tail controls and costs recall on the broad representative queries. Any ranking or API change would be a separate item gated by these numbers.

## How the fixture is built

`scripts/Generate Candidate Pool Corpus.mjs` writes `tests/fixtures/Candidate Pool Corpus-v1.json` from a seeded PRNG (mulberry32, seed 548), so it is deterministic. Vectors are synthetic and supplied by the fixture; no embedding model runs. There are 865 thoughts and 21 cases.

- **representative-small** (project `advisaur`, 48 notes plus 6 in `other-project`): six topics × four aspects × two hand-written notes. The vector is topic + 0.6 × aspect + Gaussian noise. Each query targets one topic/aspect cell, whose two notes are relevant.
- **representative-large** (project `workspace`, 320 notes; `sideline`, 64): eight topics, each with four aspect cells of four notes plus 24 general notes. Every note names its topic word and two other topic words, so a one-word topic query matches well over 100 notes. Cell notes 1–3 use the aspect phrase and note 4 paraphrases it without the aspect word. Vectors are topic + 0.6 × aspect (general notes 0.3 × a random aspect) + 0.35 per mentioned topic + Gaussian noise. Broad cases query the topic word alone, focused cases add the aspect word, and the query vector is topic + 0.6 × aspect. The relevant set is the four workspace cell notes, plus the two sideline cell notes for the all-projects cases.
- **Tail controls** (kestrel): 157 lexical-only notes contain "kestrel" and point along an axis orthogonal to the query. 157 vector-only notes omit the word. All have the same token count, so BM25 orders them by term frequency, in tiers of 24 (tf 8), 34 (tf 6), 89 (tf 4) and 10 (tf 2). The targets at ranks 25, 60 and 150 have tf 7, 5 and 3, which places each one exactly between two tiers. Vector-only notes and targets share one ordering with cosines falling strictly from 0.98 in steps of 0.003. Each target therefore holds the same rank in both sources: 25, 60 or 150.
- **Disagreement** (heron): the two relevant notes each lead one source and are absent from the other.
- **Filter control** (osprey): 70 strong distractors fail the project or type filter. The two relevant decisions are weak in both sources.

---

## Generated report

Code: `b3836ab`  
Fixture: `repository:tests/fixtures/Candidate Pool Corpus-v1.json` (candidate-pool-v1), SHA-256 `f41f300a19c55d33ec0b8d1a22680f9da10908825bbcc0b3c4867813cc656278`  
Documents: 865; vectors: fixture-supplied deterministic synthetic vectors; no embedding model (26 dimensions)  
Metric version: `retrieval-v1`; limits: 5,10; offsets: 0,limit,6*limit  
Pools per source: current=min(3*limit,100),pool-30=30,pool-100=100,pool-200=200,pool-all=865  
Override path given the production pool vs default path: 126/126 identical  
Deterministic digest: `a47ccb371ccd0edcfc9137682401ce339d349f598563ca4076f68f0fb182794f`  
Run: 2026-11-03T06:14:58Z on macos-aarch64, 1299 ms

### Mean first-page metrics: representative-small cases

Deltas are against `current` at the same limit. Median time is per handler call.

| Limit | Policy | Pool/source | Cases | Mean fused total | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | ΔRecall@5 | ΔRecall@10 | ΔNDCG@5 | ΔNDCG@10 | Median µs |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 5 | current | 15 | 6 | 11.8 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 689 |
| 5 | pool-30 | 30 | 6 | 11.8 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 438 |
| 5 | pool-100 | 100 | 6 | 11.8 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 459 |
| 5 | pool-200 | 200 | 6 | 11.8 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 470 |
| 5 | pool-all | 865 | 6 | 11.8 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 435 |
| 10 | current | 30 | 6 | 11.8 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 454 |
| 10 | pool-30 | 30 | 6 | 11.8 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 471 |
| 10 | pool-100 | 100 | 6 | 11.8 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 464 |
| 10 | pool-200 | 200 | 6 | 11.8 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 458 |
| 10 | pool-all | 865 | 6 | 11.8 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 452 |

### Mean first-page metrics: representative-large cases

Deltas are against `current` at the same limit. Median time is per handler call.

| Limit | Policy | Pool/source | Cases | Mean fused total | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | ΔRecall@5 | ΔRecall@10 | ΔNDCG@5 | ΔNDCG@10 | Median µs |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 5 | current | 15 | 10 | 21.3 | 0.5417 | 0.5417 | 0.5657 | 0.5517 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 778 |
| 5 | pool-30 | 30 | 10 | 41.3 | 0.4583 | 0.4583 | 0.5037 | 0.4929 | -0.0833 | -0.0833 | -0.0620 | -0.0588 | 725 |
| 5 | pool-100 | 100 | 10 | 103.3 | 0.4583 | 0.4583 | 0.5037 | 0.4929 | -0.0833 | -0.0833 | -0.0620 | -0.0588 | 985 |
| 5 | pool-200 | 200 | 10 | 108.8 | 0.4583 | 0.4583 | 0.5037 | 0.4929 | -0.0833 | -0.0833 | -0.0620 | -0.0588 | 1053 |
| 5 | pool-all | 865 | 10 | 108.8 | 0.4583 | 0.4583 | 0.5037 | 0.4929 | -0.0833 | -0.0833 | -0.0620 | -0.0588 | 1029 |
| 10 | current | 30 | 10 | 41.3 | 0.4583 | 0.6417 | 0.5037 | 0.5819 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 641 |
| 10 | pool-30 | 30 | 10 | 41.3 | 0.4583 | 0.6417 | 0.5037 | 0.5819 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 630 |
| 10 | pool-100 | 100 | 10 | 103.3 | 0.4583 | 0.5250 | 0.5037 | 0.5266 | +0.0000 | -0.1167 | +0.0000 | -0.0553 | 989 |
| 10 | pool-200 | 200 | 10 | 108.8 | 0.4583 | 0.5250 | 0.5037 | 0.5266 | +0.0000 | -0.1167 | +0.0000 | -0.0553 | 1087 |
| 10 | pool-all | 865 | 10 | 108.8 | 0.4583 | 0.5250 | 0.5037 | 0.5266 | +0.0000 | -0.1167 | +0.0000 | -0.0553 | 1063 |

### Mean first-page metrics: stress cases

Deltas are against `current` at the same limit. Median time is per handler call.

| Limit | Policy | Pool/source | Cases | Mean fused total | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | ΔRecall@5 | ΔRecall@10 | ΔNDCG@5 | ΔNDCG@10 | Median µs |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 5 | current | 15 | 5 | 19.6 | 0.4000 | 0.4000 | 0.4000 | 0.4000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 788 |
| 5 | pool-30 | 30 | 5 | 37.0 | 0.6000 | 0.6000 | 0.6000 | 0.6000 | +0.2000 | +0.2000 | +0.2000 | +0.2000 | 732 |
| 5 | pool-100 | 100 | 5 | 120.4 | 0.8000 | 0.8000 | 0.7262 | 0.7262 | +0.4000 | +0.4000 | +0.3262 | +0.3262 | 1270 |
| 5 | pool-200 | 200 | 5 | 191.8 | 0.8000 | 0.8000 | 0.7262 | 0.7262 | +0.4000 | +0.4000 | +0.3262 | +0.3262 | 1726 |
| 5 | pool-all | 865 | 5 | 191.8 | 0.8000 | 0.8000 | 0.7262 | 0.7262 | +0.4000 | +0.4000 | +0.3262 | +0.3262 | 1672 |
| 10 | current | 30 | 5 | 37.0 | 0.6000 | 0.6000 | 0.6000 | 0.6000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 735 |
| 10 | pool-30 | 30 | 5 | 37.0 | 0.6000 | 0.6000 | 0.6000 | 0.6000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 714 |
| 10 | pool-100 | 100 | 5 | 120.4 | 0.8000 | 0.8000 | 0.7262 | 0.7262 | +0.2000 | +0.2000 | +0.1262 | +0.1262 | 1205 |
| 10 | pool-200 | 200 | 5 | 191.8 | 0.8000 | 0.8000 | 0.7262 | 0.7262 | +0.2000 | +0.2000 | +0.1262 | +0.1262 | 1662 |
| 10 | pool-all | 865 | 5 | 191.8 | 0.8000 | 0.8000 | 0.7262 | 0.7262 | +0.2000 | +0.2000 | +0.1262 | +0.1262 | 1642 |

### First page per case

Page ranks are positions on the first page; fused ranks are positions in the whole fused pool. `-` means not returned or not admitted.

| Case | Class | Limit | Policy | Page ranks | Fused ranks | Recall@10 | NDCG@10 | Fused total | Corpus |
| --- | --- | ---: | --- | --- | --- | ---: | ---: | ---: | ---: |
| all-import-transcript | representative-large | 5 | current | ws-t7-a1-n1:5, ws-t7-a1-n2:-, ws-t7-a1-n3:-, ws-t7-a1-n4:-, sl-t7-a1-n1:-, sl-t7-a1-n2:3 | ws-t7-a1-n1:5, ws-t7-a1-n2:11, ws-t7-a1-n3:9, ws-t7-a1-n4:13, sl-t7-a1-n1:15, sl-t7-a1-n2:3 | 0.3333 | 0.2684 | 29 | 865 |
| all-import-transcript | representative-large | 5 | pool-30 | ws-t7-a1-n1:-, ws-t7-a1-n2:-, ws-t7-a1-n3:-, ws-t7-a1-n4:-, sl-t7-a1-n1:-, sl-t7-a1-n2:- | ws-t7-a1-n1:11, ws-t7-a1-n2:15, ws-t7-a1-n3:13, ws-t7-a1-n4:17, sl-t7-a1-n1:19, sl-t7-a1-n2:9 | 0.0000 | 0.0000 | 53 | 865 |
| all-import-transcript | representative-large | 5 | pool-100 | ws-t7-a1-n1:-, ws-t7-a1-n2:-, ws-t7-a1-n3:-, ws-t7-a1-n4:-, sl-t7-a1-n1:-, sl-t7-a1-n2:- | ws-t7-a1-n1:15, ws-t7-a1-n2:55, ws-t7-a1-n3:54, ws-t7-a1-n4:57, sl-t7-a1-n1:60, sl-t7-a1-n2:50 | 0.0000 | 0.0000 | 139 | 865 |
| all-import-transcript | representative-large | 5 | pool-200 | ws-t7-a1-n1:-, ws-t7-a1-n2:-, ws-t7-a1-n3:-, ws-t7-a1-n4:-, sl-t7-a1-n1:-, sl-t7-a1-n2:- | ws-t7-a1-n1:15, ws-t7-a1-n2:26, ws-t7-a1-n3:22, ws-t7-a1-n4:28, sl-t7-a1-n1:24, sl-t7-a1-n2:17 | 0.0000 | 0.0000 | 161 | 865 |
| all-import-transcript | representative-large | 5 | pool-all | ws-t7-a1-n1:-, ws-t7-a1-n2:-, ws-t7-a1-n3:-, ws-t7-a1-n4:-, sl-t7-a1-n1:-, sl-t7-a1-n2:- | ws-t7-a1-n1:15, ws-t7-a1-n2:26, ws-t7-a1-n3:22, ws-t7-a1-n4:28, sl-t7-a1-n1:24, sl-t7-a1-n2:17 | 0.0000 | 0.0000 | 161 | 865 |
| all-import-transcript | representative-large | 10 | current | ws-t7-a1-n1:-, ws-t7-a1-n2:-, ws-t7-a1-n3:-, ws-t7-a1-n4:-, sl-t7-a1-n1:-, sl-t7-a1-n2:9 | ws-t7-a1-n1:11, ws-t7-a1-n2:15, ws-t7-a1-n3:13, ws-t7-a1-n4:17, sl-t7-a1-n1:19, sl-t7-a1-n2:9 | 0.1667 | 0.0911 | 53 | 865 |
| all-import-transcript | representative-large | 10 | pool-30 | ws-t7-a1-n1:-, ws-t7-a1-n2:-, ws-t7-a1-n3:-, ws-t7-a1-n4:-, sl-t7-a1-n1:-, sl-t7-a1-n2:9 | ws-t7-a1-n1:11, ws-t7-a1-n2:15, ws-t7-a1-n3:13, ws-t7-a1-n4:17, sl-t7-a1-n1:19, sl-t7-a1-n2:9 | 0.1667 | 0.0911 | 53 | 865 |
| all-import-transcript | representative-large | 10 | pool-100 | ws-t7-a1-n1:-, ws-t7-a1-n2:-, ws-t7-a1-n3:-, ws-t7-a1-n4:-, sl-t7-a1-n1:-, sl-t7-a1-n2:- | ws-t7-a1-n1:15, ws-t7-a1-n2:55, ws-t7-a1-n3:54, ws-t7-a1-n4:57, sl-t7-a1-n1:60, sl-t7-a1-n2:50 | 0.0000 | 0.0000 | 139 | 865 |
| all-import-transcript | representative-large | 10 | pool-200 | ws-t7-a1-n1:-, ws-t7-a1-n2:-, ws-t7-a1-n3:-, ws-t7-a1-n4:-, sl-t7-a1-n1:-, sl-t7-a1-n2:- | ws-t7-a1-n1:15, ws-t7-a1-n2:26, ws-t7-a1-n3:22, ws-t7-a1-n4:28, sl-t7-a1-n1:24, sl-t7-a1-n2:17 | 0.0000 | 0.0000 | 161 | 865 |
| all-import-transcript | representative-large | 10 | pool-all | ws-t7-a1-n1:-, ws-t7-a1-n2:-, ws-t7-a1-n3:-, ws-t7-a1-n4:-, sl-t7-a1-n1:-, sl-t7-a1-n2:- | ws-t7-a1-n1:15, ws-t7-a1-n2:26, ws-t7-a1-n3:22, ws-t7-a1-n4:28, sl-t7-a1-n1:24, sl-t7-a1-n2:17 | 0.0000 | 0.0000 | 161 | 865 |
| all-sqlite-backup | representative-large | 5 | current | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:-, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:7, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | 0.8333 | 0.8922 | 16 | 865 |
| all-sqlite-backup | representative-large | 5 | pool-30 | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:-, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:7, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | 0.8333 | 0.8922 | 31 | 865 |
| all-sqlite-backup | representative-large | 5 | pool-100 | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:-, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:7, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | 0.8333 | 0.8922 | 99 | 865 |
| all-sqlite-backup | representative-large | 5 | pool-200 | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:-, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:7, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | 0.8333 | 0.8922 | 99 | 865 |
| all-sqlite-backup | representative-large | 5 | pool-all | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:-, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:7, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | 0.8333 | 0.8922 | 99 | 865 |
| all-sqlite-backup | representative-large | 10 | current | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:7, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:7, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | 1.0000 | 0.9931 | 31 | 865 |
| all-sqlite-backup | representative-large | 10 | pool-30 | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:7, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:7, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | 1.0000 | 0.9931 | 31 | 865 |
| all-sqlite-backup | representative-large | 10 | pool-100 | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:7, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:7, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | 1.0000 | 0.9931 | 99 | 865 |
| all-sqlite-backup | representative-large | 10 | pool-200 | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:7, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:7, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | 1.0000 | 0.9931 | 99 | 865 |
| all-sqlite-backup | representative-large | 10 | pool-all | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:7, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | ws-t0-a1-n1:1, ws-t0-a1-n2:3, ws-t0-a1-n3:4, ws-t0-a1-n4:7, sl-t0-a1-n1:5, sl-t0-a1-n2:2 | 1.0000 | 0.9931 | 99 | 865 |
| rep-bearer-token | representative-small | 5 | current | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-bearer-token | representative-small | 5 | pool-30 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-bearer-token | representative-small | 5 | pool-100 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-bearer-token | representative-small | 5 | pool-200 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-bearer-token | representative-small | 5 | pool-all | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-bearer-token | representative-small | 10 | current | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-bearer-token | representative-small | 10 | pool-30 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-bearer-token | representative-small | 10 | pool-100 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-bearer-token | representative-small | 10 | pool-200 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-bearer-token | representative-small | 10 | pool-all | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-checkpoint-backup | representative-small | 5 | current | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 14 | 865 |
| rep-checkpoint-backup | representative-small | 5 | pool-30 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 14 | 865 |
| rep-checkpoint-backup | representative-small | 5 | pool-100 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 14 | 865 |
| rep-checkpoint-backup | representative-small | 5 | pool-200 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 14 | 865 |
| rep-checkpoint-backup | representative-small | 5 | pool-all | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 14 | 865 |
| rep-checkpoint-backup | representative-small | 10 | current | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 14 | 865 |
| rep-checkpoint-backup | representative-small | 10 | pool-30 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 14 | 865 |
| rep-checkpoint-backup | representative-small | 10 | pool-100 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 14 | 865 |
| rep-checkpoint-backup | representative-small | 10 | pool-200 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 14 | 865 |
| rep-checkpoint-backup | representative-small | 10 | pool-all | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 14 | 865 |
| rep-project-alias | representative-small | 5 | current | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 9 | 865 |
| rep-project-alias | representative-small | 5 | pool-30 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 9 | 865 |
| rep-project-alias | representative-small | 5 | pool-100 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 9 | 865 |
| rep-project-alias | representative-small | 5 | pool-200 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 9 | 865 |
| rep-project-alias | representative-small | 5 | pool-all | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 9 | 865 |
| rep-project-alias | representative-small | 10 | current | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 9 | 865 |
| rep-project-alias | representative-small | 10 | pool-30 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 9 | 865 |
| rep-project-alias | representative-small | 10 | pool-100 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 9 | 865 |
| rep-project-alias | representative-small | 10 | pool-200 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 9 | 865 |
| rep-project-alias | representative-small | 10 | pool-all | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 9 | 865 |
| rep-rank-fusion | representative-small | 5 | current | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 865 |
| rep-rank-fusion | representative-small | 5 | pool-30 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 865 |
| rep-rank-fusion | representative-small | 5 | pool-100 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 865 |
| rep-rank-fusion | representative-small | 5 | pool-200 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 865 |
| rep-rank-fusion | representative-small | 5 | pool-all | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 865 |
| rep-rank-fusion | representative-small | 10 | current | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 865 |
| rep-rank-fusion | representative-small | 10 | pool-30 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 865 |
| rep-rank-fusion | representative-small | 10 | pool-100 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 865 |
| rep-rank-fusion | representative-small | 10 | pool-200 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 865 |
| rep-rank-fusion | representative-small | 10 | pool-all | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 865 |
| rep-similarity-threshold | representative-small | 5 | current | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 10 | 865 |
| rep-similarity-threshold | representative-small | 5 | pool-30 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 10 | 865 |
| rep-similarity-threshold | representative-small | 5 | pool-100 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 10 | 865 |
| rep-similarity-threshold | representative-small | 5 | pool-200 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 10 | 865 |
| rep-similarity-threshold | representative-small | 5 | pool-all | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 10 | 865 |
| rep-similarity-threshold | representative-small | 10 | current | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 10 | 865 |
| rep-similarity-threshold | representative-small | 10 | pool-30 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 10 | 865 |
| rep-similarity-threshold | representative-small | 10 | pool-100 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 10 | 865 |
| rep-similarity-threshold | representative-small | 10 | pool-200 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 10 | 865 |
| rep-similarity-threshold | representative-small | 10 | pool-all | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 10 | 865 |
| rep-untrusted-fence | representative-small | 5 | current | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-untrusted-fence | representative-small | 5 | pool-30 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-untrusted-fence | representative-small | 5 | pool-100 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-untrusted-fence | representative-small | 5 | pool-200 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-untrusted-fence | representative-small | 5 | pool-all | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-untrusted-fence | representative-small | 10 | current | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-untrusted-fence | representative-small | 10 | pool-30 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-untrusted-fence | representative-small | 10 | pool-100 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-untrusted-fence | representative-small | 10 | pool-200 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| rep-untrusted-fence | representative-small | 10 | pool-all | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 13 | 865 |
| stress-lexical-vector-disagreement | stress | 5 | current | heron-lexical-1:1, heron-vector-1:2 | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 865 |
| stress-lexical-vector-disagreement | stress | 5 | pool-30 | heron-lexical-1:1, heron-vector-1:2 | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 865 |
| stress-lexical-vector-disagreement | stress | 5 | pool-100 | heron-lexical-1:1, heron-vector-1:2 | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 865 |
| stress-lexical-vector-disagreement | stress | 5 | pool-200 | heron-lexical-1:1, heron-vector-1:2 | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 865 |
| stress-lexical-vector-disagreement | stress | 5 | pool-all | heron-lexical-1:1, heron-vector-1:2 | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 865 |
| stress-lexical-vector-disagreement | stress | 10 | current | heron-lexical-1:1, heron-vector-1:2 | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 865 |
| stress-lexical-vector-disagreement | stress | 10 | pool-30 | heron-lexical-1:1, heron-vector-1:2 | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 865 |
| stress-lexical-vector-disagreement | stress | 10 | pool-100 | heron-lexical-1:1, heron-vector-1:2 | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 865 |
| stress-lexical-vector-disagreement | stress | 10 | pool-200 | heron-lexical-1:1, heron-vector-1:2 | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 865 |
| stress-lexical-vector-disagreement | stress | 10 | pool-all | heron-lexical-1:1, heron-vector-1:2 | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 865 |
| stress-project-type-filter | stress | 5 | current | osprey-decision-1:2, osprey-decision-2:1 | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 865 |
| stress-project-type-filter | stress | 5 | pool-30 | osprey-decision-1:2, osprey-decision-2:1 | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 865 |
| stress-project-type-filter | stress | 5 | pool-100 | osprey-decision-1:2, osprey-decision-2:1 | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 865 |
| stress-project-type-filter | stress | 5 | pool-200 | osprey-decision-1:2, osprey-decision-2:1 | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 865 |
| stress-project-type-filter | stress | 5 | pool-all | osprey-decision-1:2, osprey-decision-2:1 | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 865 |
| stress-project-type-filter | stress | 10 | current | osprey-decision-1:2, osprey-decision-2:1 | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 865 |
| stress-project-type-filter | stress | 10 | pool-30 | osprey-decision-1:2, osprey-decision-2:1 | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 865 |
| stress-project-type-filter | stress | 10 | pool-100 | osprey-decision-1:2, osprey-decision-2:1 | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 865 |
| stress-project-type-filter | stress | 10 | pool-200 | osprey-decision-1:2, osprey-decision-2:1 | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 865 |
| stress-project-type-filter | stress | 10 | pool-all | osprey-decision-1:2, osprey-decision-2:1 | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 865 |
| stress-tail-025 | stress | 5 | current | kestrel-tail-025:- | kestrel-tail-025:- | 0.0000 | 0.0000 | 30 | 865 |
| stress-tail-025 | stress | 5 | pool-30 | kestrel-tail-025:1 | kestrel-tail-025:1 | 1.0000 | 1.0000 | 59 | 865 |
| stress-tail-025 | stress | 5 | pool-100 | kestrel-tail-025:1 | kestrel-tail-025:1 | 1.0000 | 1.0000 | 198 | 865 |
| stress-tail-025 | stress | 5 | pool-200 | kestrel-tail-025:1 | kestrel-tail-025:1 | 1.0000 | 1.0000 | 317 | 865 |
| stress-tail-025 | stress | 5 | pool-all | kestrel-tail-025:1 | kestrel-tail-025:1 | 1.0000 | 1.0000 | 317 | 865 |
| stress-tail-025 | stress | 10 | current | kestrel-tail-025:1 | kestrel-tail-025:1 | 1.0000 | 1.0000 | 59 | 865 |
| stress-tail-025 | stress | 10 | pool-30 | kestrel-tail-025:1 | kestrel-tail-025:1 | 1.0000 | 1.0000 | 59 | 865 |
| stress-tail-025 | stress | 10 | pool-100 | kestrel-tail-025:1 | kestrel-tail-025:1 | 1.0000 | 1.0000 | 198 | 865 |
| stress-tail-025 | stress | 10 | pool-200 | kestrel-tail-025:1 | kestrel-tail-025:1 | 1.0000 | 1.0000 | 317 | 865 |
| stress-tail-025 | stress | 10 | pool-all | kestrel-tail-025:1 | kestrel-tail-025:1 | 1.0000 | 1.0000 | 317 | 865 |
| stress-tail-060 | stress | 5 | current | kestrel-tail-060:- | kestrel-tail-060:- | 0.0000 | 0.0000 | 30 | 865 |
| stress-tail-060 | stress | 5 | pool-30 | kestrel-tail-060:- | kestrel-tail-060:- | 0.0000 | 0.0000 | 59 | 865 |
| stress-tail-060 | stress | 5 | pool-100 | kestrel-tail-060:2 | kestrel-tail-060:2 | 1.0000 | 0.6309 | 198 | 865 |
| stress-tail-060 | stress | 5 | pool-200 | kestrel-tail-060:2 | kestrel-tail-060:2 | 1.0000 | 0.6309 | 317 | 865 |
| stress-tail-060 | stress | 5 | pool-all | kestrel-tail-060:2 | kestrel-tail-060:2 | 1.0000 | 0.6309 | 317 | 865 |
| stress-tail-060 | stress | 10 | current | kestrel-tail-060:- | kestrel-tail-060:- | 0.0000 | 0.0000 | 59 | 865 |
| stress-tail-060 | stress | 10 | pool-30 | kestrel-tail-060:- | kestrel-tail-060:- | 0.0000 | 0.0000 | 59 | 865 |
| stress-tail-060 | stress | 10 | pool-100 | kestrel-tail-060:2 | kestrel-tail-060:2 | 1.0000 | 0.6309 | 198 | 865 |
| stress-tail-060 | stress | 10 | pool-200 | kestrel-tail-060:2 | kestrel-tail-060:2 | 1.0000 | 0.6309 | 317 | 865 |
| stress-tail-060 | stress | 10 | pool-all | kestrel-tail-060:2 | kestrel-tail-060:2 | 1.0000 | 0.6309 | 317 | 865 |
| stress-tail-150 | stress | 5 | current | kestrel-tail-150:- | kestrel-tail-150:- | 0.0000 | 0.0000 | 30 | 865 |
| stress-tail-150 | stress | 5 | pool-30 | kestrel-tail-150:- | kestrel-tail-150:- | 0.0000 | 0.0000 | 59 | 865 |
| stress-tail-150 | stress | 5 | pool-100 | kestrel-tail-150:- | kestrel-tail-150:- | 0.0000 | 0.0000 | 198 | 865 |
| stress-tail-150 | stress | 5 | pool-200 | kestrel-tail-150:- | kestrel-tail-150:90 | 0.0000 | 0.0000 | 317 | 865 |
| stress-tail-150 | stress | 5 | pool-all | kestrel-tail-150:- | kestrel-tail-150:90 | 0.0000 | 0.0000 | 317 | 865 |
| stress-tail-150 | stress | 10 | current | kestrel-tail-150:- | kestrel-tail-150:- | 0.0000 | 0.0000 | 59 | 865 |
| stress-tail-150 | stress | 10 | pool-30 | kestrel-tail-150:- | kestrel-tail-150:- | 0.0000 | 0.0000 | 59 | 865 |
| stress-tail-150 | stress | 10 | pool-100 | kestrel-tail-150:- | kestrel-tail-150:- | 0.0000 | 0.0000 | 198 | 865 |
| stress-tail-150 | stress | 10 | pool-200 | kestrel-tail-150:- | kestrel-tail-150:90 | 0.0000 | 0.0000 | 317 | 865 |
| stress-tail-150 | stress | 10 | pool-all | kestrel-tail-150:- | kestrel-tail-150:90 | 0.0000 | 0.0000 | 317 | 865 |
| ws-brief-fence | representative-large | 5 | current | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | 1.0000 | 1.0000 | 15 | 865 |
| ws-brief-fence | representative-large | 5 | pool-30 | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | 1.0000 | 1.0000 | 30 | 865 |
| ws-brief-fence | representative-large | 5 | pool-100 | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | 1.0000 | 1.0000 | 70 | 865 |
| ws-brief-fence | representative-large | 5 | pool-200 | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | 1.0000 | 1.0000 | 70 | 865 |
| ws-brief-fence | representative-large | 5 | pool-all | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | 1.0000 | 1.0000 | 70 | 865 |
| ws-brief-fence | representative-large | 10 | current | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | 1.0000 | 1.0000 | 30 | 865 |
| ws-brief-fence | representative-large | 10 | pool-30 | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | 1.0000 | 1.0000 | 30 | 865 |
| ws-brief-fence | representative-large | 10 | pool-100 | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | 1.0000 | 1.0000 | 70 | 865 |
| ws-brief-fence | representative-large | 10 | pool-200 | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | 1.0000 | 1.0000 | 70 | 865 |
| ws-brief-fence | representative-large | 10 | pool-all | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | ws-t5-a1-n1:1, ws-t5-a1-n2:3, ws-t5-a1-n3:2, ws-t5-a1-n4:4 | 1.0000 | 1.0000 | 70 | 865 |
| ws-import-watcher | representative-large | 5 | current | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:- | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:10 | 0.7500 | 0.8319 | 15 | 865 |
| ws-import-watcher | representative-large | 5 | pool-30 | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:- | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:10 | 0.7500 | 0.8319 | 30 | 865 |
| ws-import-watcher | representative-large | 5 | pool-100 | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:- | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:10 | 0.7500 | 0.8319 | 81 | 865 |
| ws-import-watcher | representative-large | 5 | pool-200 | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:- | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:10 | 0.7500 | 0.8319 | 81 | 865 |
| ws-import-watcher | representative-large | 5 | pool-all | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:- | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:10 | 0.7500 | 0.8319 | 81 | 865 |
| ws-import-watcher | representative-large | 10 | current | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:10 | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:10 | 1.0000 | 0.9447 | 30 | 865 |
| ws-import-watcher | representative-large | 10 | pool-30 | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:10 | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:10 | 1.0000 | 0.9447 | 30 | 865 |
| ws-import-watcher | representative-large | 10 | pool-100 | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:10 | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:10 | 1.0000 | 0.9447 | 81 | 865 |
| ws-import-watcher | representative-large | 10 | pool-200 | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:10 | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:10 | 1.0000 | 0.9447 | 81 | 865 |
| ws-import-watcher | representative-large | 10 | pool-all | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:10 | ws-t7-a3-n1:3, ws-t7-a3-n2:2, ws-t7-a3-n3:1, ws-t7-a3-n4:10 | 1.0000 | 0.9447 | 81 | 865 |
| ws-project-alias | representative-large | 5 | current | ws-t4-a0-n1:4, ws-t4-a0-n2:-, ws-t4-a0-n3:-, ws-t4-a0-n4:- | ws-t4-a0-n1:4, ws-t4-a0-n2:13, ws-t4-a0-n3:6, ws-t4-a0-n4:8 | 0.2500 | 0.1681 | 28 | 865 |
| ws-project-alias | representative-large | 5 | pool-30 | ws-t4-a0-n1:-, ws-t4-a0-n2:-, ws-t4-a0-n3:-, ws-t4-a0-n4:- | ws-t4-a0-n1:8, ws-t4-a0-n2:16, ws-t4-a0-n3:10, ws-t4-a0-n4:12 | 0.0000 | 0.0000 | 54 | 865 |
| ws-project-alias | representative-large | 5 | pool-100 | ws-t4-a0-n1:-, ws-t4-a0-n2:-, ws-t4-a0-n3:-, ws-t4-a0-n4:- | ws-t4-a0-n1:19, ws-t4-a0-n2:25, ws-t4-a0-n3:18, ws-t4-a0-n4:56 | 0.0000 | 0.0000 | 122 | 865 |
| ws-project-alias | representative-large | 5 | pool-200 | ws-t4-a0-n1:-, ws-t4-a0-n2:-, ws-t4-a0-n3:-, ws-t4-a0-n4:- | ws-t4-a0-n1:19, ws-t4-a0-n2:25, ws-t4-a0-n3:18, ws-t4-a0-n4:31 | 0.0000 | 0.0000 | 132 | 865 |
| ws-project-alias | representative-large | 5 | pool-all | ws-t4-a0-n1:-, ws-t4-a0-n2:-, ws-t4-a0-n3:-, ws-t4-a0-n4:- | ws-t4-a0-n1:19, ws-t4-a0-n2:25, ws-t4-a0-n3:18, ws-t4-a0-n4:31 | 0.0000 | 0.0000 | 132 | 865 |
| ws-project-alias | representative-large | 10 | current | ws-t4-a0-n1:8, ws-t4-a0-n2:-, ws-t4-a0-n3:10, ws-t4-a0-n4:- | ws-t4-a0-n1:8, ws-t4-a0-n2:16, ws-t4-a0-n3:10, ws-t4-a0-n4:12 | 0.5000 | 0.2360 | 54 | 865 |
| ws-project-alias | representative-large | 10 | pool-30 | ws-t4-a0-n1:8, ws-t4-a0-n2:-, ws-t4-a0-n3:10, ws-t4-a0-n4:- | ws-t4-a0-n1:8, ws-t4-a0-n2:16, ws-t4-a0-n3:10, ws-t4-a0-n4:12 | 0.5000 | 0.2360 | 54 | 865 |
| ws-project-alias | representative-large | 10 | pool-100 | ws-t4-a0-n1:-, ws-t4-a0-n2:-, ws-t4-a0-n3:-, ws-t4-a0-n4:- | ws-t4-a0-n1:19, ws-t4-a0-n2:25, ws-t4-a0-n3:18, ws-t4-a0-n4:56 | 0.0000 | 0.0000 | 122 | 865 |
| ws-project-alias | representative-large | 10 | pool-200 | ws-t4-a0-n1:-, ws-t4-a0-n2:-, ws-t4-a0-n3:-, ws-t4-a0-n4:- | ws-t4-a0-n1:19, ws-t4-a0-n2:25, ws-t4-a0-n3:18, ws-t4-a0-n4:31 | 0.0000 | 0.0000 | 132 | 865 |
| ws-project-alias | representative-large | 10 | pool-all | ws-t4-a0-n1:-, ws-t4-a0-n2:-, ws-t4-a0-n3:-, ws-t4-a0-n4:- | ws-t4-a0-n1:19, ws-t4-a0-n2:25, ws-t4-a0-n3:18, ws-t4-a0-n4:31 | 0.0000 | 0.0000 | 132 | 865 |
| ws-release-dmg | representative-large | 5 | current | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | 1.0000 | 0.9829 | 15 | 865 |
| ws-release-dmg | representative-large | 5 | pool-30 | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | 1.0000 | 0.9829 | 30 | 865 |
| ws-release-dmg | representative-large | 5 | pool-100 | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | 1.0000 | 0.9829 | 86 | 865 |
| ws-release-dmg | representative-large | 5 | pool-200 | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | 1.0000 | 0.9829 | 86 | 865 |
| ws-release-dmg | representative-large | 5 | pool-all | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | 1.0000 | 0.9829 | 86 | 865 |
| ws-release-dmg | representative-large | 10 | current | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | 1.0000 | 0.9829 | 30 | 865 |
| ws-release-dmg | representative-large | 10 | pool-30 | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | 1.0000 | 0.9829 | 30 | 865 |
| ws-release-dmg | representative-large | 10 | pool-100 | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | 1.0000 | 0.9829 | 86 | 865 |
| ws-release-dmg | representative-large | 10 | pool-200 | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | 1.0000 | 0.9829 | 86 | 865 |
| ws-release-dmg | representative-large | 10 | pool-all | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | ws-t3-a3-n1:2, ws-t3-a3-n2:3, ws-t3-a3-n3:1, ws-t3-a3-n4:5 | 1.0000 | 0.9829 | 86 | 865 |
| ws-search-threshold | representative-large | 5 | current | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:- | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:8 | 0.7500 | 0.8319 | 15 | 865 |
| ws-search-threshold | representative-large | 5 | pool-30 | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:- | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:8 | 0.7500 | 0.8319 | 30 | 865 |
| ws-search-threshold | representative-large | 5 | pool-100 | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:- | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:8 | 0.7500 | 0.8319 | 87 | 865 |
| ws-search-threshold | representative-large | 5 | pool-200 | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:- | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:8 | 0.7500 | 0.8319 | 87 | 865 |
| ws-search-threshold | representative-large | 5 | pool-all | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:- | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:8 | 0.7500 | 0.8319 | 87 | 865 |
| ws-search-threshold | representative-large | 10 | current | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:8 | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:8 | 1.0000 | 0.9550 | 30 | 865 |
| ws-search-threshold | representative-large | 10 | pool-30 | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:8 | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:8 | 1.0000 | 0.9550 | 30 | 865 |
| ws-search-threshold | representative-large | 10 | pool-100 | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:8 | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:8 | 1.0000 | 0.9550 | 87 | 865 |
| ws-search-threshold | representative-large | 10 | pool-200 | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:8 | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:8 | 1.0000 | 0.9550 | 87 | 865 |
| ws-search-threshold | representative-large | 10 | pool-all | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:8 | ws-t1-a1-n1:3, ws-t1-a1-n2:2, ws-t1-a1-n3:1, ws-t1-a1-n4:8 | 1.0000 | 0.9550 | 87 | 865 |
| ws-sqlite-wal | representative-large | 5 | current | ws-t0-a0-n1:-, ws-t0-a0-n2:-, ws-t0-a0-n3:5, ws-t0-a0-n4:- | ws-t0-a0-n1:13, ws-t0-a0-n2:15, ws-t0-a0-n3:5, ws-t0-a0-n4:9 | 0.2500 | 0.1510 | 27 | 865 |
| ws-sqlite-wal | representative-large | 5 | pool-30 | ws-t0-a0-n1:-, ws-t0-a0-n2:-, ws-t0-a0-n3:-, ws-t0-a0-n4:- | ws-t0-a0-n1:16, ws-t0-a0-n2:18, ws-t0-a0-n3:10, ws-t0-a0-n4:13 | 0.0000 | 0.0000 | 51 | 865 |
| ws-sqlite-wal | representative-large | 5 | pool-100 | ws-t0-a0-n1:-, ws-t0-a0-n2:-, ws-t0-a0-n3:-, ws-t0-a0-n4:- | ws-t0-a0-n1:13, ws-t0-a0-n2:24, ws-t0-a0-n3:14, ws-t0-a0-n4:62 | 0.0000 | 0.0000 | 114 | 865 |
| ws-sqlite-wal | representative-large | 5 | pool-200 | ws-t0-a0-n1:-, ws-t0-a0-n2:-, ws-t0-a0-n3:-, ws-t0-a0-n4:- | ws-t0-a0-n1:13, ws-t0-a0-n2:25, ws-t0-a0-n3:14, ws-t0-a0-n4:23 | 0.0000 | 0.0000 | 119 | 865 |
| ws-sqlite-wal | representative-large | 5 | pool-all | ws-t0-a0-n1:-, ws-t0-a0-n2:-, ws-t0-a0-n3:-, ws-t0-a0-n4:- | ws-t0-a0-n1:13, ws-t0-a0-n2:25, ws-t0-a0-n3:14, ws-t0-a0-n4:23 | 0.0000 | 0.0000 | 119 | 865 |
| ws-sqlite-wal | representative-large | 10 | current | ws-t0-a0-n1:-, ws-t0-a0-n2:-, ws-t0-a0-n3:10, ws-t0-a0-n4:- | ws-t0-a0-n1:16, ws-t0-a0-n2:18, ws-t0-a0-n3:10, ws-t0-a0-n4:13 | 0.2500 | 0.1128 | 51 | 865 |
| ws-sqlite-wal | representative-large | 10 | pool-30 | ws-t0-a0-n1:-, ws-t0-a0-n2:-, ws-t0-a0-n3:10, ws-t0-a0-n4:- | ws-t0-a0-n1:16, ws-t0-a0-n2:18, ws-t0-a0-n3:10, ws-t0-a0-n4:13 | 0.2500 | 0.1128 | 51 | 865 |
| ws-sqlite-wal | representative-large | 10 | pool-100 | ws-t0-a0-n1:-, ws-t0-a0-n2:-, ws-t0-a0-n3:-, ws-t0-a0-n4:- | ws-t0-a0-n1:13, ws-t0-a0-n2:24, ws-t0-a0-n3:14, ws-t0-a0-n4:62 | 0.0000 | 0.0000 | 114 | 865 |
| ws-sqlite-wal | representative-large | 10 | pool-200 | ws-t0-a0-n1:-, ws-t0-a0-n2:-, ws-t0-a0-n3:-, ws-t0-a0-n4:- | ws-t0-a0-n1:13, ws-t0-a0-n2:25, ws-t0-a0-n3:14, ws-t0-a0-n4:23 | 0.0000 | 0.0000 | 119 | 865 |
| ws-sqlite-wal | representative-large | 10 | pool-all | ws-t0-a0-n1:-, ws-t0-a0-n2:-, ws-t0-a0-n3:-, ws-t0-a0-n4:- | ws-t0-a0-n1:13, ws-t0-a0-n2:25, ws-t0-a0-n3:14, ws-t0-a0-n4:23 | 0.0000 | 0.0000 | 119 | 865 |
| ws-sync-offline | representative-large | 5 | current | ws-t6-a2-n1:-, ws-t6-a2-n2:-, ws-t6-a2-n3:-, ws-t6-a2-n4:- | ws-t6-a2-n1:10, ws-t6-a2-n2:8, ws-t6-a2-n3:6, ws-t6-a2-n4:12 | 0.0000 | 0.0000 | 28 | 865 |
| ws-sync-offline | representative-large | 5 | pool-30 | ws-t6-a2-n1:-, ws-t6-a2-n2:-, ws-t6-a2-n3:-, ws-t6-a2-n4:- | ws-t6-a2-n1:14, ws-t6-a2-n2:12, ws-t6-a2-n3:10, ws-t6-a2-n4:15 | 0.0000 | 0.0000 | 54 | 865 |
| ws-sync-offline | representative-large | 5 | pool-100 | ws-t6-a2-n1:-, ws-t6-a2-n2:-, ws-t6-a2-n3:-, ws-t6-a2-n4:- | ws-t6-a2-n1:19, ws-t6-a2-n2:24, ws-t6-a2-n3:17, ws-t6-a2-n4:63 | 0.0000 | 0.0000 | 115 | 865 |
| ws-sync-offline | representative-large | 5 | pool-200 | ws-t6-a2-n1:-, ws-t6-a2-n2:-, ws-t6-a2-n3:-, ws-t6-a2-n4:- | ws-t6-a2-n1:19, ws-t6-a2-n2:24, ws-t6-a2-n3:17, ws-t6-a2-n4:32 | 0.0000 | 0.0000 | 127 | 865 |
| ws-sync-offline | representative-large | 5 | pool-all | ws-t6-a2-n1:-, ws-t6-a2-n2:-, ws-t6-a2-n3:-, ws-t6-a2-n4:- | ws-t6-a2-n1:19, ws-t6-a2-n2:24, ws-t6-a2-n3:17, ws-t6-a2-n4:32 | 0.0000 | 0.0000 | 127 | 865 |
| ws-sync-offline | representative-large | 10 | current | ws-t6-a2-n1:-, ws-t6-a2-n2:-, ws-t6-a2-n3:10, ws-t6-a2-n4:- | ws-t6-a2-n1:14, ws-t6-a2-n2:12, ws-t6-a2-n3:10, ws-t6-a2-n4:15 | 0.2500 | 0.1128 | 54 | 865 |
| ws-sync-offline | representative-large | 10 | pool-30 | ws-t6-a2-n1:-, ws-t6-a2-n2:-, ws-t6-a2-n3:10, ws-t6-a2-n4:- | ws-t6-a2-n1:14, ws-t6-a2-n2:12, ws-t6-a2-n3:10, ws-t6-a2-n4:15 | 0.2500 | 0.1128 | 54 | 865 |
| ws-sync-offline | representative-large | 10 | pool-100 | ws-t6-a2-n1:-, ws-t6-a2-n2:-, ws-t6-a2-n3:-, ws-t6-a2-n4:- | ws-t6-a2-n1:19, ws-t6-a2-n2:24, ws-t6-a2-n3:17, ws-t6-a2-n4:63 | 0.0000 | 0.0000 | 115 | 865 |
| ws-sync-offline | representative-large | 10 | pool-200 | ws-t6-a2-n1:-, ws-t6-a2-n2:-, ws-t6-a2-n3:-, ws-t6-a2-n4:- | ws-t6-a2-n1:19, ws-t6-a2-n2:24, ws-t6-a2-n3:17, ws-t6-a2-n4:32 | 0.0000 | 0.0000 | 127 | 865 |
| ws-sync-offline | representative-large | 10 | pool-all | ws-t6-a2-n1:-, ws-t6-a2-n2:-, ws-t6-a2-n3:-, ws-t6-a2-n4:- | ws-t6-a2-n1:19, ws-t6-a2-n2:24, ws-t6-a2-n3:17, ws-t6-a2-n4:32 | 0.0000 | 0.0000 | 127 | 865 |
| ws-token-keychain | representative-large | 5 | current | ws-t2-a2-n1:1, ws-t2-a2-n2:-, ws-t2-a2-n3:-, ws-t2-a2-n4:- | ws-t2-a2-n1:1, ws-t2-a2-n2:14, ws-t2-a2-n3:15, ws-t2-a2-n4:8 | 0.2500 | 0.3904 | 25 | 865 |
| ws-token-keychain | representative-large | 5 | pool-30 | ws-t2-a2-n1:1, ws-t2-a2-n2:-, ws-t2-a2-n3:-, ws-t2-a2-n4:- | ws-t2-a2-n1:1, ws-t2-a2-n2:18, ws-t2-a2-n3:19, ws-t2-a2-n4:13 | 0.2500 | 0.3904 | 50 | 865 |
| ws-token-keychain | representative-large | 5 | pool-100 | ws-t2-a2-n1:1, ws-t2-a2-n2:-, ws-t2-a2-n3:-, ws-t2-a2-n4:- | ws-t2-a2-n1:1, ws-t2-a2-n2:62, ws-t2-a2-n3:25, ws-t2-a2-n4:14 | 0.2500 | 0.3904 | 120 | 865 |
| ws-token-keychain | representative-large | 5 | pool-200 | ws-t2-a2-n1:1, ws-t2-a2-n2:-, ws-t2-a2-n3:-, ws-t2-a2-n4:- | ws-t2-a2-n1:1, ws-t2-a2-n2:27, ws-t2-a2-n3:25, ws-t2-a2-n4:14 | 0.2500 | 0.3904 | 126 | 865 |
| ws-token-keychain | representative-large | 5 | pool-all | ws-t2-a2-n1:1, ws-t2-a2-n2:-, ws-t2-a2-n3:-, ws-t2-a2-n4:- | ws-t2-a2-n1:1, ws-t2-a2-n2:27, ws-t2-a2-n3:25, ws-t2-a2-n4:14 | 0.2500 | 0.3904 | 126 | 865 |
| ws-token-keychain | representative-large | 10 | current | ws-t2-a2-n1:1, ws-t2-a2-n2:-, ws-t2-a2-n3:-, ws-t2-a2-n4:- | ws-t2-a2-n1:1, ws-t2-a2-n2:18, ws-t2-a2-n3:19, ws-t2-a2-n4:13 | 0.2500 | 0.3904 | 50 | 865 |
| ws-token-keychain | representative-large | 10 | pool-30 | ws-t2-a2-n1:1, ws-t2-a2-n2:-, ws-t2-a2-n3:-, ws-t2-a2-n4:- | ws-t2-a2-n1:1, ws-t2-a2-n2:18, ws-t2-a2-n3:19, ws-t2-a2-n4:13 | 0.2500 | 0.3904 | 50 | 865 |
| ws-token-keychain | representative-large | 10 | pool-100 | ws-t2-a2-n1:1, ws-t2-a2-n2:-, ws-t2-a2-n3:-, ws-t2-a2-n4:- | ws-t2-a2-n1:1, ws-t2-a2-n2:62, ws-t2-a2-n3:25, ws-t2-a2-n4:14 | 0.2500 | 0.3904 | 120 | 865 |
| ws-token-keychain | representative-large | 10 | pool-200 | ws-t2-a2-n1:1, ws-t2-a2-n2:-, ws-t2-a2-n3:-, ws-t2-a2-n4:- | ws-t2-a2-n1:1, ws-t2-a2-n2:27, ws-t2-a2-n3:25, ws-t2-a2-n4:14 | 0.2500 | 0.3904 | 126 | 865 |
| ws-token-keychain | representative-large | 10 | pool-all | ws-t2-a2-n1:1, ws-t2-a2-n2:-, ws-t2-a2-n3:-, ws-t2-a2-n4:- | ws-t2-a2-n1:1, ws-t2-a2-n2:27, ws-t2-a2-n3:25, ws-t2-a2-n4:14 | 0.2500 | 0.3904 | 126 | 865 |

### Pagination

`total_count` is the filtered fused pool, not the corpus or the full match set.

| Case | Limit | Policy | Offset | Page size | total_count | has_more | Corpus |
| --- | ---: | --- | ---: | ---: | ---: | --- | ---: |
| all-import-transcript | 5 | current | 0 | 5 | 29 | true | 865 |
| all-import-transcript | 5 | current | 5 | 5 | 29 | true | 865 |
| all-import-transcript | 5 | current | 30 | 0 | 29 | false | 865 |
| all-import-transcript | 5 | pool-30 | 0 | 5 | 53 | true | 865 |
| all-import-transcript | 5 | pool-30 | 5 | 5 | 53 | true | 865 |
| all-import-transcript | 5 | pool-30 | 30 | 5 | 53 | true | 865 |
| all-import-transcript | 5 | pool-100 | 0 | 5 | 139 | true | 865 |
| all-import-transcript | 5 | pool-100 | 5 | 5 | 139 | true | 865 |
| all-import-transcript | 5 | pool-100 | 30 | 5 | 139 | true | 865 |
| all-import-transcript | 5 | pool-200 | 0 | 5 | 161 | true | 865 |
| all-import-transcript | 5 | pool-200 | 5 | 5 | 161 | true | 865 |
| all-import-transcript | 5 | pool-200 | 30 | 5 | 161 | true | 865 |
| all-import-transcript | 5 | pool-all | 0 | 5 | 161 | true | 865 |
| all-import-transcript | 5 | pool-all | 5 | 5 | 161 | true | 865 |
| all-import-transcript | 5 | pool-all | 30 | 5 | 161 | true | 865 |
| all-import-transcript | 10 | current | 0 | 10 | 53 | true | 865 |
| all-import-transcript | 10 | current | 10 | 10 | 53 | true | 865 |
| all-import-transcript | 10 | current | 60 | 0 | 53 | false | 865 |
| all-import-transcript | 10 | pool-30 | 0 | 10 | 53 | true | 865 |
| all-import-transcript | 10 | pool-30 | 10 | 10 | 53 | true | 865 |
| all-import-transcript | 10 | pool-30 | 60 | 0 | 53 | false | 865 |
| all-import-transcript | 10 | pool-100 | 0 | 10 | 139 | true | 865 |
| all-import-transcript | 10 | pool-100 | 10 | 10 | 139 | true | 865 |
| all-import-transcript | 10 | pool-100 | 60 | 10 | 139 | true | 865 |
| all-import-transcript | 10 | pool-200 | 0 | 10 | 161 | true | 865 |
| all-import-transcript | 10 | pool-200 | 10 | 10 | 161 | true | 865 |
| all-import-transcript | 10 | pool-200 | 60 | 10 | 161 | true | 865 |
| all-import-transcript | 10 | pool-all | 0 | 10 | 161 | true | 865 |
| all-import-transcript | 10 | pool-all | 10 | 10 | 161 | true | 865 |
| all-import-transcript | 10 | pool-all | 60 | 10 | 161 | true | 865 |
| all-sqlite-backup | 5 | current | 0 | 5 | 16 | true | 865 |
| all-sqlite-backup | 5 | current | 5 | 5 | 16 | true | 865 |
| all-sqlite-backup | 5 | current | 30 | 0 | 16 | false | 865 |
| all-sqlite-backup | 5 | pool-30 | 0 | 5 | 31 | true | 865 |
| all-sqlite-backup | 5 | pool-30 | 5 | 5 | 31 | true | 865 |
| all-sqlite-backup | 5 | pool-30 | 30 | 1 | 31 | false | 865 |
| all-sqlite-backup | 5 | pool-100 | 0 | 5 | 99 | true | 865 |
| all-sqlite-backup | 5 | pool-100 | 5 | 5 | 99 | true | 865 |
| all-sqlite-backup | 5 | pool-100 | 30 | 5 | 99 | true | 865 |
| all-sqlite-backup | 5 | pool-200 | 0 | 5 | 99 | true | 865 |
| all-sqlite-backup | 5 | pool-200 | 5 | 5 | 99 | true | 865 |
| all-sqlite-backup | 5 | pool-200 | 30 | 5 | 99 | true | 865 |
| all-sqlite-backup | 5 | pool-all | 0 | 5 | 99 | true | 865 |
| all-sqlite-backup | 5 | pool-all | 5 | 5 | 99 | true | 865 |
| all-sqlite-backup | 5 | pool-all | 30 | 5 | 99 | true | 865 |
| all-sqlite-backup | 10 | current | 0 | 10 | 31 | true | 865 |
| all-sqlite-backup | 10 | current | 10 | 10 | 31 | true | 865 |
| all-sqlite-backup | 10 | current | 60 | 0 | 31 | false | 865 |
| all-sqlite-backup | 10 | pool-30 | 0 | 10 | 31 | true | 865 |
| all-sqlite-backup | 10 | pool-30 | 10 | 10 | 31 | true | 865 |
| all-sqlite-backup | 10 | pool-30 | 60 | 0 | 31 | false | 865 |
| all-sqlite-backup | 10 | pool-100 | 0 | 10 | 99 | true | 865 |
| all-sqlite-backup | 10 | pool-100 | 10 | 10 | 99 | true | 865 |
| all-sqlite-backup | 10 | pool-100 | 60 | 10 | 99 | true | 865 |
| all-sqlite-backup | 10 | pool-200 | 0 | 10 | 99 | true | 865 |
| all-sqlite-backup | 10 | pool-200 | 10 | 10 | 99 | true | 865 |
| all-sqlite-backup | 10 | pool-200 | 60 | 10 | 99 | true | 865 |
| all-sqlite-backup | 10 | pool-all | 0 | 10 | 99 | true | 865 |
| all-sqlite-backup | 10 | pool-all | 10 | 10 | 99 | true | 865 |
| all-sqlite-backup | 10 | pool-all | 60 | 10 | 99 | true | 865 |
| rep-bearer-token | 5 | current | 0 | 5 | 13 | true | 865 |
| rep-bearer-token | 5 | current | 5 | 5 | 13 | true | 865 |
| rep-bearer-token | 5 | current | 30 | 0 | 13 | false | 865 |
| rep-bearer-token | 5 | pool-30 | 0 | 5 | 13 | true | 865 |
| rep-bearer-token | 5 | pool-30 | 5 | 5 | 13 | true | 865 |
| rep-bearer-token | 5 | pool-30 | 30 | 0 | 13 | false | 865 |
| rep-bearer-token | 5 | pool-100 | 0 | 5 | 13 | true | 865 |
| rep-bearer-token | 5 | pool-100 | 5 | 5 | 13 | true | 865 |
| rep-bearer-token | 5 | pool-100 | 30 | 0 | 13 | false | 865 |
| rep-bearer-token | 5 | pool-200 | 0 | 5 | 13 | true | 865 |
| rep-bearer-token | 5 | pool-200 | 5 | 5 | 13 | true | 865 |
| rep-bearer-token | 5 | pool-200 | 30 | 0 | 13 | false | 865 |
| rep-bearer-token | 5 | pool-all | 0 | 5 | 13 | true | 865 |
| rep-bearer-token | 5 | pool-all | 5 | 5 | 13 | true | 865 |
| rep-bearer-token | 5 | pool-all | 30 | 0 | 13 | false | 865 |
| rep-bearer-token | 10 | current | 0 | 10 | 13 | true | 865 |
| rep-bearer-token | 10 | current | 10 | 3 | 13 | false | 865 |
| rep-bearer-token | 10 | current | 60 | 0 | 13 | false | 865 |
| rep-bearer-token | 10 | pool-30 | 0 | 10 | 13 | true | 865 |
| rep-bearer-token | 10 | pool-30 | 10 | 3 | 13 | false | 865 |
| rep-bearer-token | 10 | pool-30 | 60 | 0 | 13 | false | 865 |
| rep-bearer-token | 10 | pool-100 | 0 | 10 | 13 | true | 865 |
| rep-bearer-token | 10 | pool-100 | 10 | 3 | 13 | false | 865 |
| rep-bearer-token | 10 | pool-100 | 60 | 0 | 13 | false | 865 |
| rep-bearer-token | 10 | pool-200 | 0 | 10 | 13 | true | 865 |
| rep-bearer-token | 10 | pool-200 | 10 | 3 | 13 | false | 865 |
| rep-bearer-token | 10 | pool-200 | 60 | 0 | 13 | false | 865 |
| rep-bearer-token | 10 | pool-all | 0 | 10 | 13 | true | 865 |
| rep-bearer-token | 10 | pool-all | 10 | 3 | 13 | false | 865 |
| rep-bearer-token | 10 | pool-all | 60 | 0 | 13 | false | 865 |
| rep-checkpoint-backup | 5 | current | 0 | 5 | 14 | true | 865 |
| rep-checkpoint-backup | 5 | current | 5 | 5 | 14 | true | 865 |
| rep-checkpoint-backup | 5 | current | 30 | 0 | 14 | false | 865 |
| rep-checkpoint-backup | 5 | pool-30 | 0 | 5 | 14 | true | 865 |
| rep-checkpoint-backup | 5 | pool-30 | 5 | 5 | 14 | true | 865 |
| rep-checkpoint-backup | 5 | pool-30 | 30 | 0 | 14 | false | 865 |
| rep-checkpoint-backup | 5 | pool-100 | 0 | 5 | 14 | true | 865 |
| rep-checkpoint-backup | 5 | pool-100 | 5 | 5 | 14 | true | 865 |
| rep-checkpoint-backup | 5 | pool-100 | 30 | 0 | 14 | false | 865 |
| rep-checkpoint-backup | 5 | pool-200 | 0 | 5 | 14 | true | 865 |
| rep-checkpoint-backup | 5 | pool-200 | 5 | 5 | 14 | true | 865 |
| rep-checkpoint-backup | 5 | pool-200 | 30 | 0 | 14 | false | 865 |
| rep-checkpoint-backup | 5 | pool-all | 0 | 5 | 14 | true | 865 |
| rep-checkpoint-backup | 5 | pool-all | 5 | 5 | 14 | true | 865 |
| rep-checkpoint-backup | 5 | pool-all | 30 | 0 | 14 | false | 865 |
| rep-checkpoint-backup | 10 | current | 0 | 10 | 14 | true | 865 |
| rep-checkpoint-backup | 10 | current | 10 | 4 | 14 | false | 865 |
| rep-checkpoint-backup | 10 | current | 60 | 0 | 14 | false | 865 |
| rep-checkpoint-backup | 10 | pool-30 | 0 | 10 | 14 | true | 865 |
| rep-checkpoint-backup | 10 | pool-30 | 10 | 4 | 14 | false | 865 |
| rep-checkpoint-backup | 10 | pool-30 | 60 | 0 | 14 | false | 865 |
| rep-checkpoint-backup | 10 | pool-100 | 0 | 10 | 14 | true | 865 |
| rep-checkpoint-backup | 10 | pool-100 | 10 | 4 | 14 | false | 865 |
| rep-checkpoint-backup | 10 | pool-100 | 60 | 0 | 14 | false | 865 |
| rep-checkpoint-backup | 10 | pool-200 | 0 | 10 | 14 | true | 865 |
| rep-checkpoint-backup | 10 | pool-200 | 10 | 4 | 14 | false | 865 |
| rep-checkpoint-backup | 10 | pool-200 | 60 | 0 | 14 | false | 865 |
| rep-checkpoint-backup | 10 | pool-all | 0 | 10 | 14 | true | 865 |
| rep-checkpoint-backup | 10 | pool-all | 10 | 4 | 14 | false | 865 |
| rep-checkpoint-backup | 10 | pool-all | 60 | 0 | 14 | false | 865 |
| rep-project-alias | 5 | current | 0 | 5 | 9 | true | 865 |
| rep-project-alias | 5 | current | 5 | 4 | 9 | false | 865 |
| rep-project-alias | 5 | current | 30 | 0 | 9 | false | 865 |
| rep-project-alias | 5 | pool-30 | 0 | 5 | 9 | true | 865 |
| rep-project-alias | 5 | pool-30 | 5 | 4 | 9 | false | 865 |
| rep-project-alias | 5 | pool-30 | 30 | 0 | 9 | false | 865 |
| rep-project-alias | 5 | pool-100 | 0 | 5 | 9 | true | 865 |
| rep-project-alias | 5 | pool-100 | 5 | 4 | 9 | false | 865 |
| rep-project-alias | 5 | pool-100 | 30 | 0 | 9 | false | 865 |
| rep-project-alias | 5 | pool-200 | 0 | 5 | 9 | true | 865 |
| rep-project-alias | 5 | pool-200 | 5 | 4 | 9 | false | 865 |
| rep-project-alias | 5 | pool-200 | 30 | 0 | 9 | false | 865 |
| rep-project-alias | 5 | pool-all | 0 | 5 | 9 | true | 865 |
| rep-project-alias | 5 | pool-all | 5 | 4 | 9 | false | 865 |
| rep-project-alias | 5 | pool-all | 30 | 0 | 9 | false | 865 |
| rep-project-alias | 10 | current | 0 | 9 | 9 | false | 865 |
| rep-project-alias | 10 | current | 10 | 0 | 9 | false | 865 |
| rep-project-alias | 10 | current | 60 | 0 | 9 | false | 865 |
| rep-project-alias | 10 | pool-30 | 0 | 9 | 9 | false | 865 |
| rep-project-alias | 10 | pool-30 | 10 | 0 | 9 | false | 865 |
| rep-project-alias | 10 | pool-30 | 60 | 0 | 9 | false | 865 |
| rep-project-alias | 10 | pool-100 | 0 | 9 | 9 | false | 865 |
| rep-project-alias | 10 | pool-100 | 10 | 0 | 9 | false | 865 |
| rep-project-alias | 10 | pool-100 | 60 | 0 | 9 | false | 865 |
| rep-project-alias | 10 | pool-200 | 0 | 9 | 9 | false | 865 |
| rep-project-alias | 10 | pool-200 | 10 | 0 | 9 | false | 865 |
| rep-project-alias | 10 | pool-200 | 60 | 0 | 9 | false | 865 |
| rep-project-alias | 10 | pool-all | 0 | 9 | 9 | false | 865 |
| rep-project-alias | 10 | pool-all | 10 | 0 | 9 | false | 865 |
| rep-project-alias | 10 | pool-all | 60 | 0 | 9 | false | 865 |
| rep-rank-fusion | 5 | current | 0 | 5 | 12 | true | 865 |
| rep-rank-fusion | 5 | current | 5 | 5 | 12 | true | 865 |
| rep-rank-fusion | 5 | current | 30 | 0 | 12 | false | 865 |
| rep-rank-fusion | 5 | pool-30 | 0 | 5 | 12 | true | 865 |
| rep-rank-fusion | 5 | pool-30 | 5 | 5 | 12 | true | 865 |
| rep-rank-fusion | 5 | pool-30 | 30 | 0 | 12 | false | 865 |
| rep-rank-fusion | 5 | pool-100 | 0 | 5 | 12 | true | 865 |
| rep-rank-fusion | 5 | pool-100 | 5 | 5 | 12 | true | 865 |
| rep-rank-fusion | 5 | pool-100 | 30 | 0 | 12 | false | 865 |
| rep-rank-fusion | 5 | pool-200 | 0 | 5 | 12 | true | 865 |
| rep-rank-fusion | 5 | pool-200 | 5 | 5 | 12 | true | 865 |
| rep-rank-fusion | 5 | pool-200 | 30 | 0 | 12 | false | 865 |
| rep-rank-fusion | 5 | pool-all | 0 | 5 | 12 | true | 865 |
| rep-rank-fusion | 5 | pool-all | 5 | 5 | 12 | true | 865 |
| rep-rank-fusion | 5 | pool-all | 30 | 0 | 12 | false | 865 |
| rep-rank-fusion | 10 | current | 0 | 10 | 12 | true | 865 |
| rep-rank-fusion | 10 | current | 10 | 2 | 12 | false | 865 |
| rep-rank-fusion | 10 | current | 60 | 0 | 12 | false | 865 |
| rep-rank-fusion | 10 | pool-30 | 0 | 10 | 12 | true | 865 |
| rep-rank-fusion | 10 | pool-30 | 10 | 2 | 12 | false | 865 |
| rep-rank-fusion | 10 | pool-30 | 60 | 0 | 12 | false | 865 |
| rep-rank-fusion | 10 | pool-100 | 0 | 10 | 12 | true | 865 |
| rep-rank-fusion | 10 | pool-100 | 10 | 2 | 12 | false | 865 |
| rep-rank-fusion | 10 | pool-100 | 60 | 0 | 12 | false | 865 |
| rep-rank-fusion | 10 | pool-200 | 0 | 10 | 12 | true | 865 |
| rep-rank-fusion | 10 | pool-200 | 10 | 2 | 12 | false | 865 |
| rep-rank-fusion | 10 | pool-200 | 60 | 0 | 12 | false | 865 |
| rep-rank-fusion | 10 | pool-all | 0 | 10 | 12 | true | 865 |
| rep-rank-fusion | 10 | pool-all | 10 | 2 | 12 | false | 865 |
| rep-rank-fusion | 10 | pool-all | 60 | 0 | 12 | false | 865 |
| rep-similarity-threshold | 5 | current | 0 | 5 | 10 | true | 865 |
| rep-similarity-threshold | 5 | current | 5 | 5 | 10 | false | 865 |
| rep-similarity-threshold | 5 | current | 30 | 0 | 10 | false | 865 |
| rep-similarity-threshold | 5 | pool-30 | 0 | 5 | 10 | true | 865 |
| rep-similarity-threshold | 5 | pool-30 | 5 | 5 | 10 | false | 865 |
| rep-similarity-threshold | 5 | pool-30 | 30 | 0 | 10 | false | 865 |
| rep-similarity-threshold | 5 | pool-100 | 0 | 5 | 10 | true | 865 |
| rep-similarity-threshold | 5 | pool-100 | 5 | 5 | 10 | false | 865 |
| rep-similarity-threshold | 5 | pool-100 | 30 | 0 | 10 | false | 865 |
| rep-similarity-threshold | 5 | pool-200 | 0 | 5 | 10 | true | 865 |
| rep-similarity-threshold | 5 | pool-200 | 5 | 5 | 10 | false | 865 |
| rep-similarity-threshold | 5 | pool-200 | 30 | 0 | 10 | false | 865 |
| rep-similarity-threshold | 5 | pool-all | 0 | 5 | 10 | true | 865 |
| rep-similarity-threshold | 5 | pool-all | 5 | 5 | 10 | false | 865 |
| rep-similarity-threshold | 5 | pool-all | 30 | 0 | 10 | false | 865 |
| rep-similarity-threshold | 10 | current | 0 | 10 | 10 | false | 865 |
| rep-similarity-threshold | 10 | current | 10 | 0 | 10 | false | 865 |
| rep-similarity-threshold | 10 | current | 60 | 0 | 10 | false | 865 |
| rep-similarity-threshold | 10 | pool-30 | 0 | 10 | 10 | false | 865 |
| rep-similarity-threshold | 10 | pool-30 | 10 | 0 | 10 | false | 865 |
| rep-similarity-threshold | 10 | pool-30 | 60 | 0 | 10 | false | 865 |
| rep-similarity-threshold | 10 | pool-100 | 0 | 10 | 10 | false | 865 |
| rep-similarity-threshold | 10 | pool-100 | 10 | 0 | 10 | false | 865 |
| rep-similarity-threshold | 10 | pool-100 | 60 | 0 | 10 | false | 865 |
| rep-similarity-threshold | 10 | pool-200 | 0 | 10 | 10 | false | 865 |
| rep-similarity-threshold | 10 | pool-200 | 10 | 0 | 10 | false | 865 |
| rep-similarity-threshold | 10 | pool-200 | 60 | 0 | 10 | false | 865 |
| rep-similarity-threshold | 10 | pool-all | 0 | 10 | 10 | false | 865 |
| rep-similarity-threshold | 10 | pool-all | 10 | 0 | 10 | false | 865 |
| rep-similarity-threshold | 10 | pool-all | 60 | 0 | 10 | false | 865 |
| rep-untrusted-fence | 5 | current | 0 | 5 | 13 | true | 865 |
| rep-untrusted-fence | 5 | current | 5 | 5 | 13 | true | 865 |
| rep-untrusted-fence | 5 | current | 30 | 0 | 13 | false | 865 |
| rep-untrusted-fence | 5 | pool-30 | 0 | 5 | 13 | true | 865 |
| rep-untrusted-fence | 5 | pool-30 | 5 | 5 | 13 | true | 865 |
| rep-untrusted-fence | 5 | pool-30 | 30 | 0 | 13 | false | 865 |
| rep-untrusted-fence | 5 | pool-100 | 0 | 5 | 13 | true | 865 |
| rep-untrusted-fence | 5 | pool-100 | 5 | 5 | 13 | true | 865 |
| rep-untrusted-fence | 5 | pool-100 | 30 | 0 | 13 | false | 865 |
| rep-untrusted-fence | 5 | pool-200 | 0 | 5 | 13 | true | 865 |
| rep-untrusted-fence | 5 | pool-200 | 5 | 5 | 13 | true | 865 |
| rep-untrusted-fence | 5 | pool-200 | 30 | 0 | 13 | false | 865 |
| rep-untrusted-fence | 5 | pool-all | 0 | 5 | 13 | true | 865 |
| rep-untrusted-fence | 5 | pool-all | 5 | 5 | 13 | true | 865 |
| rep-untrusted-fence | 5 | pool-all | 30 | 0 | 13 | false | 865 |
| rep-untrusted-fence | 10 | current | 0 | 10 | 13 | true | 865 |
| rep-untrusted-fence | 10 | current | 10 | 3 | 13 | false | 865 |
| rep-untrusted-fence | 10 | current | 60 | 0 | 13 | false | 865 |
| rep-untrusted-fence | 10 | pool-30 | 0 | 10 | 13 | true | 865 |
| rep-untrusted-fence | 10 | pool-30 | 10 | 3 | 13 | false | 865 |
| rep-untrusted-fence | 10 | pool-30 | 60 | 0 | 13 | false | 865 |
| rep-untrusted-fence | 10 | pool-100 | 0 | 10 | 13 | true | 865 |
| rep-untrusted-fence | 10 | pool-100 | 10 | 3 | 13 | false | 865 |
| rep-untrusted-fence | 10 | pool-100 | 60 | 0 | 13 | false | 865 |
| rep-untrusted-fence | 10 | pool-200 | 0 | 10 | 13 | true | 865 |
| rep-untrusted-fence | 10 | pool-200 | 10 | 3 | 13 | false | 865 |
| rep-untrusted-fence | 10 | pool-200 | 60 | 0 | 13 | false | 865 |
| rep-untrusted-fence | 10 | pool-all | 0 | 10 | 13 | true | 865 |
| rep-untrusted-fence | 10 | pool-all | 10 | 3 | 13 | false | 865 |
| rep-untrusted-fence | 10 | pool-all | 60 | 0 | 13 | false | 865 |
| stress-lexical-vector-disagreement | 5 | current | 0 | 5 | 6 | true | 865 |
| stress-lexical-vector-disagreement | 5 | current | 5 | 1 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 5 | current | 30 | 0 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 5 | pool-30 | 0 | 5 | 6 | true | 865 |
| stress-lexical-vector-disagreement | 5 | pool-30 | 5 | 1 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 5 | pool-30 | 30 | 0 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 5 | pool-100 | 0 | 5 | 6 | true | 865 |
| stress-lexical-vector-disagreement | 5 | pool-100 | 5 | 1 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 5 | pool-100 | 30 | 0 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 5 | pool-200 | 0 | 5 | 6 | true | 865 |
| stress-lexical-vector-disagreement | 5 | pool-200 | 5 | 1 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 5 | pool-200 | 30 | 0 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 5 | pool-all | 0 | 5 | 6 | true | 865 |
| stress-lexical-vector-disagreement | 5 | pool-all | 5 | 1 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 5 | pool-all | 30 | 0 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 10 | current | 0 | 6 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 10 | current | 10 | 0 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 10 | current | 60 | 0 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 10 | pool-30 | 0 | 6 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 10 | pool-30 | 10 | 0 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 10 | pool-30 | 60 | 0 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 10 | pool-100 | 0 | 6 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 10 | pool-100 | 10 | 0 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 10 | pool-100 | 60 | 0 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 10 | pool-200 | 0 | 6 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 10 | pool-200 | 10 | 0 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 10 | pool-200 | 60 | 0 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 10 | pool-all | 0 | 6 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 10 | pool-all | 10 | 0 | 6 | false | 865 |
| stress-lexical-vector-disagreement | 10 | pool-all | 60 | 0 | 6 | false | 865 |
| stress-project-type-filter | 5 | current | 0 | 2 | 2 | false | 865 |
| stress-project-type-filter | 5 | current | 5 | 0 | 2 | false | 865 |
| stress-project-type-filter | 5 | current | 30 | 0 | 2 | false | 865 |
| stress-project-type-filter | 5 | pool-30 | 0 | 2 | 2 | false | 865 |
| stress-project-type-filter | 5 | pool-30 | 5 | 0 | 2 | false | 865 |
| stress-project-type-filter | 5 | pool-30 | 30 | 0 | 2 | false | 865 |
| stress-project-type-filter | 5 | pool-100 | 0 | 2 | 2 | false | 865 |
| stress-project-type-filter | 5 | pool-100 | 5 | 0 | 2 | false | 865 |
| stress-project-type-filter | 5 | pool-100 | 30 | 0 | 2 | false | 865 |
| stress-project-type-filter | 5 | pool-200 | 0 | 2 | 2 | false | 865 |
| stress-project-type-filter | 5 | pool-200 | 5 | 0 | 2 | false | 865 |
| stress-project-type-filter | 5 | pool-200 | 30 | 0 | 2 | false | 865 |
| stress-project-type-filter | 5 | pool-all | 0 | 2 | 2 | false | 865 |
| stress-project-type-filter | 5 | pool-all | 5 | 0 | 2 | false | 865 |
| stress-project-type-filter | 5 | pool-all | 30 | 0 | 2 | false | 865 |
| stress-project-type-filter | 10 | current | 0 | 2 | 2 | false | 865 |
| stress-project-type-filter | 10 | current | 10 | 0 | 2 | false | 865 |
| stress-project-type-filter | 10 | current | 60 | 0 | 2 | false | 865 |
| stress-project-type-filter | 10 | pool-30 | 0 | 2 | 2 | false | 865 |
| stress-project-type-filter | 10 | pool-30 | 10 | 0 | 2 | false | 865 |
| stress-project-type-filter | 10 | pool-30 | 60 | 0 | 2 | false | 865 |
| stress-project-type-filter | 10 | pool-100 | 0 | 2 | 2 | false | 865 |
| stress-project-type-filter | 10 | pool-100 | 10 | 0 | 2 | false | 865 |
| stress-project-type-filter | 10 | pool-100 | 60 | 0 | 2 | false | 865 |
| stress-project-type-filter | 10 | pool-200 | 0 | 2 | 2 | false | 865 |
| stress-project-type-filter | 10 | pool-200 | 10 | 0 | 2 | false | 865 |
| stress-project-type-filter | 10 | pool-200 | 60 | 0 | 2 | false | 865 |
| stress-project-type-filter | 10 | pool-all | 0 | 2 | 2 | false | 865 |
| stress-project-type-filter | 10 | pool-all | 10 | 0 | 2 | false | 865 |
| stress-project-type-filter | 10 | pool-all | 60 | 0 | 2 | false | 865 |
| stress-tail-025 | 5 | current | 0 | 5 | 30 | true | 865 |
| stress-tail-025 | 5 | current | 5 | 5 | 30 | true | 865 |
| stress-tail-025 | 5 | current | 30 | 0 | 30 | false | 865 |
| stress-tail-025 | 5 | pool-30 | 0 | 5 | 59 | true | 865 |
| stress-tail-025 | 5 | pool-30 | 5 | 5 | 59 | true | 865 |
| stress-tail-025 | 5 | pool-30 | 30 | 5 | 59 | true | 865 |
| stress-tail-025 | 5 | pool-100 | 0 | 5 | 198 | true | 865 |
| stress-tail-025 | 5 | pool-100 | 5 | 5 | 198 | true | 865 |
| stress-tail-025 | 5 | pool-100 | 30 | 5 | 198 | true | 865 |
| stress-tail-025 | 5 | pool-200 | 0 | 5 | 317 | true | 865 |
| stress-tail-025 | 5 | pool-200 | 5 | 5 | 317 | true | 865 |
| stress-tail-025 | 5 | pool-200 | 30 | 5 | 317 | true | 865 |
| stress-tail-025 | 5 | pool-all | 0 | 5 | 317 | true | 865 |
| stress-tail-025 | 5 | pool-all | 5 | 5 | 317 | true | 865 |
| stress-tail-025 | 5 | pool-all | 30 | 5 | 317 | true | 865 |
| stress-tail-025 | 10 | current | 0 | 10 | 59 | true | 865 |
| stress-tail-025 | 10 | current | 10 | 10 | 59 | true | 865 |
| stress-tail-025 | 10 | current | 60 | 0 | 59 | false | 865 |
| stress-tail-025 | 10 | pool-30 | 0 | 10 | 59 | true | 865 |
| stress-tail-025 | 10 | pool-30 | 10 | 10 | 59 | true | 865 |
| stress-tail-025 | 10 | pool-30 | 60 | 0 | 59 | false | 865 |
| stress-tail-025 | 10 | pool-100 | 0 | 10 | 198 | true | 865 |
| stress-tail-025 | 10 | pool-100 | 10 | 10 | 198 | true | 865 |
| stress-tail-025 | 10 | pool-100 | 60 | 10 | 198 | true | 865 |
| stress-tail-025 | 10 | pool-200 | 0 | 10 | 317 | true | 865 |
| stress-tail-025 | 10 | pool-200 | 10 | 10 | 317 | true | 865 |
| stress-tail-025 | 10 | pool-200 | 60 | 10 | 317 | true | 865 |
| stress-tail-025 | 10 | pool-all | 0 | 10 | 317 | true | 865 |
| stress-tail-025 | 10 | pool-all | 10 | 10 | 317 | true | 865 |
| stress-tail-025 | 10 | pool-all | 60 | 10 | 317 | true | 865 |
| stress-tail-060 | 5 | current | 0 | 5 | 30 | true | 865 |
| stress-tail-060 | 5 | current | 5 | 5 | 30 | true | 865 |
| stress-tail-060 | 5 | current | 30 | 0 | 30 | false | 865 |
| stress-tail-060 | 5 | pool-30 | 0 | 5 | 59 | true | 865 |
| stress-tail-060 | 5 | pool-30 | 5 | 5 | 59 | true | 865 |
| stress-tail-060 | 5 | pool-30 | 30 | 5 | 59 | true | 865 |
| stress-tail-060 | 5 | pool-100 | 0 | 5 | 198 | true | 865 |
| stress-tail-060 | 5 | pool-100 | 5 | 5 | 198 | true | 865 |
| stress-tail-060 | 5 | pool-100 | 30 | 5 | 198 | true | 865 |
| stress-tail-060 | 5 | pool-200 | 0 | 5 | 317 | true | 865 |
| stress-tail-060 | 5 | pool-200 | 5 | 5 | 317 | true | 865 |
| stress-tail-060 | 5 | pool-200 | 30 | 5 | 317 | true | 865 |
| stress-tail-060 | 5 | pool-all | 0 | 5 | 317 | true | 865 |
| stress-tail-060 | 5 | pool-all | 5 | 5 | 317 | true | 865 |
| stress-tail-060 | 5 | pool-all | 30 | 5 | 317 | true | 865 |
| stress-tail-060 | 10 | current | 0 | 10 | 59 | true | 865 |
| stress-tail-060 | 10 | current | 10 | 10 | 59 | true | 865 |
| stress-tail-060 | 10 | current | 60 | 0 | 59 | false | 865 |
| stress-tail-060 | 10 | pool-30 | 0 | 10 | 59 | true | 865 |
| stress-tail-060 | 10 | pool-30 | 10 | 10 | 59 | true | 865 |
| stress-tail-060 | 10 | pool-30 | 60 | 0 | 59 | false | 865 |
| stress-tail-060 | 10 | pool-100 | 0 | 10 | 198 | true | 865 |
| stress-tail-060 | 10 | pool-100 | 10 | 10 | 198 | true | 865 |
| stress-tail-060 | 10 | pool-100 | 60 | 10 | 198 | true | 865 |
| stress-tail-060 | 10 | pool-200 | 0 | 10 | 317 | true | 865 |
| stress-tail-060 | 10 | pool-200 | 10 | 10 | 317 | true | 865 |
| stress-tail-060 | 10 | pool-200 | 60 | 10 | 317 | true | 865 |
| stress-tail-060 | 10 | pool-all | 0 | 10 | 317 | true | 865 |
| stress-tail-060 | 10 | pool-all | 10 | 10 | 317 | true | 865 |
| stress-tail-060 | 10 | pool-all | 60 | 10 | 317 | true | 865 |
| stress-tail-150 | 5 | current | 0 | 5 | 30 | true | 865 |
| stress-tail-150 | 5 | current | 5 | 5 | 30 | true | 865 |
| stress-tail-150 | 5 | current | 30 | 0 | 30 | false | 865 |
| stress-tail-150 | 5 | pool-30 | 0 | 5 | 59 | true | 865 |
| stress-tail-150 | 5 | pool-30 | 5 | 5 | 59 | true | 865 |
| stress-tail-150 | 5 | pool-30 | 30 | 5 | 59 | true | 865 |
| stress-tail-150 | 5 | pool-100 | 0 | 5 | 198 | true | 865 |
| stress-tail-150 | 5 | pool-100 | 5 | 5 | 198 | true | 865 |
| stress-tail-150 | 5 | pool-100 | 30 | 5 | 198 | true | 865 |
| stress-tail-150 | 5 | pool-200 | 0 | 5 | 317 | true | 865 |
| stress-tail-150 | 5 | pool-200 | 5 | 5 | 317 | true | 865 |
| stress-tail-150 | 5 | pool-200 | 30 | 5 | 317 | true | 865 |
| stress-tail-150 | 5 | pool-all | 0 | 5 | 317 | true | 865 |
| stress-tail-150 | 5 | pool-all | 5 | 5 | 317 | true | 865 |
| stress-tail-150 | 5 | pool-all | 30 | 5 | 317 | true | 865 |
| stress-tail-150 | 10 | current | 0 | 10 | 59 | true | 865 |
| stress-tail-150 | 10 | current | 10 | 10 | 59 | true | 865 |
| stress-tail-150 | 10 | current | 60 | 0 | 59 | false | 865 |
| stress-tail-150 | 10 | pool-30 | 0 | 10 | 59 | true | 865 |
| stress-tail-150 | 10 | pool-30 | 10 | 10 | 59 | true | 865 |
| stress-tail-150 | 10 | pool-30 | 60 | 0 | 59 | false | 865 |
| stress-tail-150 | 10 | pool-100 | 0 | 10 | 198 | true | 865 |
| stress-tail-150 | 10 | pool-100 | 10 | 10 | 198 | true | 865 |
| stress-tail-150 | 10 | pool-100 | 60 | 10 | 198 | true | 865 |
| stress-tail-150 | 10 | pool-200 | 0 | 10 | 317 | true | 865 |
| stress-tail-150 | 10 | pool-200 | 10 | 10 | 317 | true | 865 |
| stress-tail-150 | 10 | pool-200 | 60 | 10 | 317 | true | 865 |
| stress-tail-150 | 10 | pool-all | 0 | 10 | 317 | true | 865 |
| stress-tail-150 | 10 | pool-all | 10 | 10 | 317 | true | 865 |
| stress-tail-150 | 10 | pool-all | 60 | 10 | 317 | true | 865 |
| ws-brief-fence | 5 | current | 0 | 5 | 15 | true | 865 |
| ws-brief-fence | 5 | current | 5 | 5 | 15 | true | 865 |
| ws-brief-fence | 5 | current | 30 | 0 | 15 | false | 865 |
| ws-brief-fence | 5 | pool-30 | 0 | 5 | 30 | true | 865 |
| ws-brief-fence | 5 | pool-30 | 5 | 5 | 30 | true | 865 |
| ws-brief-fence | 5 | pool-30 | 30 | 0 | 30 | false | 865 |
| ws-brief-fence | 5 | pool-100 | 0 | 5 | 70 | true | 865 |
| ws-brief-fence | 5 | pool-100 | 5 | 5 | 70 | true | 865 |
| ws-brief-fence | 5 | pool-100 | 30 | 5 | 70 | true | 865 |
| ws-brief-fence | 5 | pool-200 | 0 | 5 | 70 | true | 865 |
| ws-brief-fence | 5 | pool-200 | 5 | 5 | 70 | true | 865 |
| ws-brief-fence | 5 | pool-200 | 30 | 5 | 70 | true | 865 |
| ws-brief-fence | 5 | pool-all | 0 | 5 | 70 | true | 865 |
| ws-brief-fence | 5 | pool-all | 5 | 5 | 70 | true | 865 |
| ws-brief-fence | 5 | pool-all | 30 | 5 | 70 | true | 865 |
| ws-brief-fence | 10 | current | 0 | 10 | 30 | true | 865 |
| ws-brief-fence | 10 | current | 10 | 10 | 30 | true | 865 |
| ws-brief-fence | 10 | current | 60 | 0 | 30 | false | 865 |
| ws-brief-fence | 10 | pool-30 | 0 | 10 | 30 | true | 865 |
| ws-brief-fence | 10 | pool-30 | 10 | 10 | 30 | true | 865 |
| ws-brief-fence | 10 | pool-30 | 60 | 0 | 30 | false | 865 |
| ws-brief-fence | 10 | pool-100 | 0 | 10 | 70 | true | 865 |
| ws-brief-fence | 10 | pool-100 | 10 | 10 | 70 | true | 865 |
| ws-brief-fence | 10 | pool-100 | 60 | 10 | 70 | false | 865 |
| ws-brief-fence | 10 | pool-200 | 0 | 10 | 70 | true | 865 |
| ws-brief-fence | 10 | pool-200 | 10 | 10 | 70 | true | 865 |
| ws-brief-fence | 10 | pool-200 | 60 | 10 | 70 | false | 865 |
| ws-brief-fence | 10 | pool-all | 0 | 10 | 70 | true | 865 |
| ws-brief-fence | 10 | pool-all | 10 | 10 | 70 | true | 865 |
| ws-brief-fence | 10 | pool-all | 60 | 10 | 70 | false | 865 |
| ws-import-watcher | 5 | current | 0 | 5 | 15 | true | 865 |
| ws-import-watcher | 5 | current | 5 | 5 | 15 | true | 865 |
| ws-import-watcher | 5 | current | 30 | 0 | 15 | false | 865 |
| ws-import-watcher | 5 | pool-30 | 0 | 5 | 30 | true | 865 |
| ws-import-watcher | 5 | pool-30 | 5 | 5 | 30 | true | 865 |
| ws-import-watcher | 5 | pool-30 | 30 | 0 | 30 | false | 865 |
| ws-import-watcher | 5 | pool-100 | 0 | 5 | 81 | true | 865 |
| ws-import-watcher | 5 | pool-100 | 5 | 5 | 81 | true | 865 |
| ws-import-watcher | 5 | pool-100 | 30 | 5 | 81 | true | 865 |
| ws-import-watcher | 5 | pool-200 | 0 | 5 | 81 | true | 865 |
| ws-import-watcher | 5 | pool-200 | 5 | 5 | 81 | true | 865 |
| ws-import-watcher | 5 | pool-200 | 30 | 5 | 81 | true | 865 |
| ws-import-watcher | 5 | pool-all | 0 | 5 | 81 | true | 865 |
| ws-import-watcher | 5 | pool-all | 5 | 5 | 81 | true | 865 |
| ws-import-watcher | 5 | pool-all | 30 | 5 | 81 | true | 865 |
| ws-import-watcher | 10 | current | 0 | 10 | 30 | true | 865 |
| ws-import-watcher | 10 | current | 10 | 10 | 30 | true | 865 |
| ws-import-watcher | 10 | current | 60 | 0 | 30 | false | 865 |
| ws-import-watcher | 10 | pool-30 | 0 | 10 | 30 | true | 865 |
| ws-import-watcher | 10 | pool-30 | 10 | 10 | 30 | true | 865 |
| ws-import-watcher | 10 | pool-30 | 60 | 0 | 30 | false | 865 |
| ws-import-watcher | 10 | pool-100 | 0 | 10 | 81 | true | 865 |
| ws-import-watcher | 10 | pool-100 | 10 | 10 | 81 | true | 865 |
| ws-import-watcher | 10 | pool-100 | 60 | 10 | 81 | true | 865 |
| ws-import-watcher | 10 | pool-200 | 0 | 10 | 81 | true | 865 |
| ws-import-watcher | 10 | pool-200 | 10 | 10 | 81 | true | 865 |
| ws-import-watcher | 10 | pool-200 | 60 | 10 | 81 | true | 865 |
| ws-import-watcher | 10 | pool-all | 0 | 10 | 81 | true | 865 |
| ws-import-watcher | 10 | pool-all | 10 | 10 | 81 | true | 865 |
| ws-import-watcher | 10 | pool-all | 60 | 10 | 81 | true | 865 |
| ws-project-alias | 5 | current | 0 | 5 | 28 | true | 865 |
| ws-project-alias | 5 | current | 5 | 5 | 28 | true | 865 |
| ws-project-alias | 5 | current | 30 | 0 | 28 | false | 865 |
| ws-project-alias | 5 | pool-30 | 0 | 5 | 54 | true | 865 |
| ws-project-alias | 5 | pool-30 | 5 | 5 | 54 | true | 865 |
| ws-project-alias | 5 | pool-30 | 30 | 5 | 54 | true | 865 |
| ws-project-alias | 5 | pool-100 | 0 | 5 | 122 | true | 865 |
| ws-project-alias | 5 | pool-100 | 5 | 5 | 122 | true | 865 |
| ws-project-alias | 5 | pool-100 | 30 | 5 | 122 | true | 865 |
| ws-project-alias | 5 | pool-200 | 0 | 5 | 132 | true | 865 |
| ws-project-alias | 5 | pool-200 | 5 | 5 | 132 | true | 865 |
| ws-project-alias | 5 | pool-200 | 30 | 5 | 132 | true | 865 |
| ws-project-alias | 5 | pool-all | 0 | 5 | 132 | true | 865 |
| ws-project-alias | 5 | pool-all | 5 | 5 | 132 | true | 865 |
| ws-project-alias | 5 | pool-all | 30 | 5 | 132 | true | 865 |
| ws-project-alias | 10 | current | 0 | 10 | 54 | true | 865 |
| ws-project-alias | 10 | current | 10 | 10 | 54 | true | 865 |
| ws-project-alias | 10 | current | 60 | 0 | 54 | false | 865 |
| ws-project-alias | 10 | pool-30 | 0 | 10 | 54 | true | 865 |
| ws-project-alias | 10 | pool-30 | 10 | 10 | 54 | true | 865 |
| ws-project-alias | 10 | pool-30 | 60 | 0 | 54 | false | 865 |
| ws-project-alias | 10 | pool-100 | 0 | 10 | 122 | true | 865 |
| ws-project-alias | 10 | pool-100 | 10 | 10 | 122 | true | 865 |
| ws-project-alias | 10 | pool-100 | 60 | 10 | 122 | true | 865 |
| ws-project-alias | 10 | pool-200 | 0 | 10 | 132 | true | 865 |
| ws-project-alias | 10 | pool-200 | 10 | 10 | 132 | true | 865 |
| ws-project-alias | 10 | pool-200 | 60 | 10 | 132 | true | 865 |
| ws-project-alias | 10 | pool-all | 0 | 10 | 132 | true | 865 |
| ws-project-alias | 10 | pool-all | 10 | 10 | 132 | true | 865 |
| ws-project-alias | 10 | pool-all | 60 | 10 | 132 | true | 865 |
| ws-release-dmg | 5 | current | 0 | 5 | 15 | true | 865 |
| ws-release-dmg | 5 | current | 5 | 5 | 15 | true | 865 |
| ws-release-dmg | 5 | current | 30 | 0 | 15 | false | 865 |
| ws-release-dmg | 5 | pool-30 | 0 | 5 | 30 | true | 865 |
| ws-release-dmg | 5 | pool-30 | 5 | 5 | 30 | true | 865 |
| ws-release-dmg | 5 | pool-30 | 30 | 0 | 30 | false | 865 |
| ws-release-dmg | 5 | pool-100 | 0 | 5 | 86 | true | 865 |
| ws-release-dmg | 5 | pool-100 | 5 | 5 | 86 | true | 865 |
| ws-release-dmg | 5 | pool-100 | 30 | 5 | 86 | true | 865 |
| ws-release-dmg | 5 | pool-200 | 0 | 5 | 86 | true | 865 |
| ws-release-dmg | 5 | pool-200 | 5 | 5 | 86 | true | 865 |
| ws-release-dmg | 5 | pool-200 | 30 | 5 | 86 | true | 865 |
| ws-release-dmg | 5 | pool-all | 0 | 5 | 86 | true | 865 |
| ws-release-dmg | 5 | pool-all | 5 | 5 | 86 | true | 865 |
| ws-release-dmg | 5 | pool-all | 30 | 5 | 86 | true | 865 |
| ws-release-dmg | 10 | current | 0 | 10 | 30 | true | 865 |
| ws-release-dmg | 10 | current | 10 | 10 | 30 | true | 865 |
| ws-release-dmg | 10 | current | 60 | 0 | 30 | false | 865 |
| ws-release-dmg | 10 | pool-30 | 0 | 10 | 30 | true | 865 |
| ws-release-dmg | 10 | pool-30 | 10 | 10 | 30 | true | 865 |
| ws-release-dmg | 10 | pool-30 | 60 | 0 | 30 | false | 865 |
| ws-release-dmg | 10 | pool-100 | 0 | 10 | 86 | true | 865 |
| ws-release-dmg | 10 | pool-100 | 10 | 10 | 86 | true | 865 |
| ws-release-dmg | 10 | pool-100 | 60 | 10 | 86 | true | 865 |
| ws-release-dmg | 10 | pool-200 | 0 | 10 | 86 | true | 865 |
| ws-release-dmg | 10 | pool-200 | 10 | 10 | 86 | true | 865 |
| ws-release-dmg | 10 | pool-200 | 60 | 10 | 86 | true | 865 |
| ws-release-dmg | 10 | pool-all | 0 | 10 | 86 | true | 865 |
| ws-release-dmg | 10 | pool-all | 10 | 10 | 86 | true | 865 |
| ws-release-dmg | 10 | pool-all | 60 | 10 | 86 | true | 865 |
| ws-search-threshold | 5 | current | 0 | 5 | 15 | true | 865 |
| ws-search-threshold | 5 | current | 5 | 5 | 15 | true | 865 |
| ws-search-threshold | 5 | current | 30 | 0 | 15 | false | 865 |
| ws-search-threshold | 5 | pool-30 | 0 | 5 | 30 | true | 865 |
| ws-search-threshold | 5 | pool-30 | 5 | 5 | 30 | true | 865 |
| ws-search-threshold | 5 | pool-30 | 30 | 0 | 30 | false | 865 |
| ws-search-threshold | 5 | pool-100 | 0 | 5 | 87 | true | 865 |
| ws-search-threshold | 5 | pool-100 | 5 | 5 | 87 | true | 865 |
| ws-search-threshold | 5 | pool-100 | 30 | 5 | 87 | true | 865 |
| ws-search-threshold | 5 | pool-200 | 0 | 5 | 87 | true | 865 |
| ws-search-threshold | 5 | pool-200 | 5 | 5 | 87 | true | 865 |
| ws-search-threshold | 5 | pool-200 | 30 | 5 | 87 | true | 865 |
| ws-search-threshold | 5 | pool-all | 0 | 5 | 87 | true | 865 |
| ws-search-threshold | 5 | pool-all | 5 | 5 | 87 | true | 865 |
| ws-search-threshold | 5 | pool-all | 30 | 5 | 87 | true | 865 |
| ws-search-threshold | 10 | current | 0 | 10 | 30 | true | 865 |
| ws-search-threshold | 10 | current | 10 | 10 | 30 | true | 865 |
| ws-search-threshold | 10 | current | 60 | 0 | 30 | false | 865 |
| ws-search-threshold | 10 | pool-30 | 0 | 10 | 30 | true | 865 |
| ws-search-threshold | 10 | pool-30 | 10 | 10 | 30 | true | 865 |
| ws-search-threshold | 10 | pool-30 | 60 | 0 | 30 | false | 865 |
| ws-search-threshold | 10 | pool-100 | 0 | 10 | 87 | true | 865 |
| ws-search-threshold | 10 | pool-100 | 10 | 10 | 87 | true | 865 |
| ws-search-threshold | 10 | pool-100 | 60 | 10 | 87 | true | 865 |
| ws-search-threshold | 10 | pool-200 | 0 | 10 | 87 | true | 865 |
| ws-search-threshold | 10 | pool-200 | 10 | 10 | 87 | true | 865 |
| ws-search-threshold | 10 | pool-200 | 60 | 10 | 87 | true | 865 |
| ws-search-threshold | 10 | pool-all | 0 | 10 | 87 | true | 865 |
| ws-search-threshold | 10 | pool-all | 10 | 10 | 87 | true | 865 |
| ws-search-threshold | 10 | pool-all | 60 | 10 | 87 | true | 865 |
| ws-sqlite-wal | 5 | current | 0 | 5 | 27 | true | 865 |
| ws-sqlite-wal | 5 | current | 5 | 5 | 27 | true | 865 |
| ws-sqlite-wal | 5 | current | 30 | 0 | 27 | false | 865 |
| ws-sqlite-wal | 5 | pool-30 | 0 | 5 | 51 | true | 865 |
| ws-sqlite-wal | 5 | pool-30 | 5 | 5 | 51 | true | 865 |
| ws-sqlite-wal | 5 | pool-30 | 30 | 5 | 51 | true | 865 |
| ws-sqlite-wal | 5 | pool-100 | 0 | 5 | 114 | true | 865 |
| ws-sqlite-wal | 5 | pool-100 | 5 | 5 | 114 | true | 865 |
| ws-sqlite-wal | 5 | pool-100 | 30 | 5 | 114 | true | 865 |
| ws-sqlite-wal | 5 | pool-200 | 0 | 5 | 119 | true | 865 |
| ws-sqlite-wal | 5 | pool-200 | 5 | 5 | 119 | true | 865 |
| ws-sqlite-wal | 5 | pool-200 | 30 | 5 | 119 | true | 865 |
| ws-sqlite-wal | 5 | pool-all | 0 | 5 | 119 | true | 865 |
| ws-sqlite-wal | 5 | pool-all | 5 | 5 | 119 | true | 865 |
| ws-sqlite-wal | 5 | pool-all | 30 | 5 | 119 | true | 865 |
| ws-sqlite-wal | 10 | current | 0 | 10 | 51 | true | 865 |
| ws-sqlite-wal | 10 | current | 10 | 10 | 51 | true | 865 |
| ws-sqlite-wal | 10 | current | 60 | 0 | 51 | false | 865 |
| ws-sqlite-wal | 10 | pool-30 | 0 | 10 | 51 | true | 865 |
| ws-sqlite-wal | 10 | pool-30 | 10 | 10 | 51 | true | 865 |
| ws-sqlite-wal | 10 | pool-30 | 60 | 0 | 51 | false | 865 |
| ws-sqlite-wal | 10 | pool-100 | 0 | 10 | 114 | true | 865 |
| ws-sqlite-wal | 10 | pool-100 | 10 | 10 | 114 | true | 865 |
| ws-sqlite-wal | 10 | pool-100 | 60 | 10 | 114 | true | 865 |
| ws-sqlite-wal | 10 | pool-200 | 0 | 10 | 119 | true | 865 |
| ws-sqlite-wal | 10 | pool-200 | 10 | 10 | 119 | true | 865 |
| ws-sqlite-wal | 10 | pool-200 | 60 | 10 | 119 | true | 865 |
| ws-sqlite-wal | 10 | pool-all | 0 | 10 | 119 | true | 865 |
| ws-sqlite-wal | 10 | pool-all | 10 | 10 | 119 | true | 865 |
| ws-sqlite-wal | 10 | pool-all | 60 | 10 | 119 | true | 865 |
| ws-sync-offline | 5 | current | 0 | 5 | 28 | true | 865 |
| ws-sync-offline | 5 | current | 5 | 5 | 28 | true | 865 |
| ws-sync-offline | 5 | current | 30 | 0 | 28 | false | 865 |
| ws-sync-offline | 5 | pool-30 | 0 | 5 | 54 | true | 865 |
| ws-sync-offline | 5 | pool-30 | 5 | 5 | 54 | true | 865 |
| ws-sync-offline | 5 | pool-30 | 30 | 5 | 54 | true | 865 |
| ws-sync-offline | 5 | pool-100 | 0 | 5 | 115 | true | 865 |
| ws-sync-offline | 5 | pool-100 | 5 | 5 | 115 | true | 865 |
| ws-sync-offline | 5 | pool-100 | 30 | 5 | 115 | true | 865 |
| ws-sync-offline | 5 | pool-200 | 0 | 5 | 127 | true | 865 |
| ws-sync-offline | 5 | pool-200 | 5 | 5 | 127 | true | 865 |
| ws-sync-offline | 5 | pool-200 | 30 | 5 | 127 | true | 865 |
| ws-sync-offline | 5 | pool-all | 0 | 5 | 127 | true | 865 |
| ws-sync-offline | 5 | pool-all | 5 | 5 | 127 | true | 865 |
| ws-sync-offline | 5 | pool-all | 30 | 5 | 127 | true | 865 |
| ws-sync-offline | 10 | current | 0 | 10 | 54 | true | 865 |
| ws-sync-offline | 10 | current | 10 | 10 | 54 | true | 865 |
| ws-sync-offline | 10 | current | 60 | 0 | 54 | false | 865 |
| ws-sync-offline | 10 | pool-30 | 0 | 10 | 54 | true | 865 |
| ws-sync-offline | 10 | pool-30 | 10 | 10 | 54 | true | 865 |
| ws-sync-offline | 10 | pool-30 | 60 | 0 | 54 | false | 865 |
| ws-sync-offline | 10 | pool-100 | 0 | 10 | 115 | true | 865 |
| ws-sync-offline | 10 | pool-100 | 10 | 10 | 115 | true | 865 |
| ws-sync-offline | 10 | pool-100 | 60 | 10 | 115 | true | 865 |
| ws-sync-offline | 10 | pool-200 | 0 | 10 | 127 | true | 865 |
| ws-sync-offline | 10 | pool-200 | 10 | 10 | 127 | true | 865 |
| ws-sync-offline | 10 | pool-200 | 60 | 10 | 127 | true | 865 |
| ws-sync-offline | 10 | pool-all | 0 | 10 | 127 | true | 865 |
| ws-sync-offline | 10 | pool-all | 10 | 10 | 127 | true | 865 |
| ws-sync-offline | 10 | pool-all | 60 | 10 | 127 | true | 865 |
| ws-token-keychain | 5 | current | 0 | 5 | 25 | true | 865 |
| ws-token-keychain | 5 | current | 5 | 5 | 25 | true | 865 |
| ws-token-keychain | 5 | current | 30 | 0 | 25 | false | 865 |
| ws-token-keychain | 5 | pool-30 | 0 | 5 | 50 | true | 865 |
| ws-token-keychain | 5 | pool-30 | 5 | 5 | 50 | true | 865 |
| ws-token-keychain | 5 | pool-30 | 30 | 5 | 50 | true | 865 |
| ws-token-keychain | 5 | pool-100 | 0 | 5 | 120 | true | 865 |
| ws-token-keychain | 5 | pool-100 | 5 | 5 | 120 | true | 865 |
| ws-token-keychain | 5 | pool-100 | 30 | 5 | 120 | true | 865 |
| ws-token-keychain | 5 | pool-200 | 0 | 5 | 126 | true | 865 |
| ws-token-keychain | 5 | pool-200 | 5 | 5 | 126 | true | 865 |
| ws-token-keychain | 5 | pool-200 | 30 | 5 | 126 | true | 865 |
| ws-token-keychain | 5 | pool-all | 0 | 5 | 126 | true | 865 |
| ws-token-keychain | 5 | pool-all | 5 | 5 | 126 | true | 865 |
| ws-token-keychain | 5 | pool-all | 30 | 5 | 126 | true | 865 |
| ws-token-keychain | 10 | current | 0 | 10 | 50 | true | 865 |
| ws-token-keychain | 10 | current | 10 | 10 | 50 | true | 865 |
| ws-token-keychain | 10 | current | 60 | 0 | 50 | false | 865 |
| ws-token-keychain | 10 | pool-30 | 0 | 10 | 50 | true | 865 |
| ws-token-keychain | 10 | pool-30 | 10 | 10 | 50 | true | 865 |
| ws-token-keychain | 10 | pool-30 | 60 | 0 | 50 | false | 865 |
| ws-token-keychain | 10 | pool-100 | 0 | 10 | 120 | true | 865 |
| ws-token-keychain | 10 | pool-100 | 10 | 10 | 120 | true | 865 |
| ws-token-keychain | 10 | pool-100 | 60 | 10 | 120 | true | 865 |
| ws-token-keychain | 10 | pool-200 | 0 | 10 | 126 | true | 865 |
| ws-token-keychain | 10 | pool-200 | 10 | 10 | 126 | true | 865 |
| ws-token-keychain | 10 | pool-200 | 60 | 10 | 126 | true | 865 |
| ws-token-keychain | 10 | pool-all | 0 | 10 | 126 | true | 865 |
| ws-token-keychain | 10 | pool-all | 10 | 10 | 126 | true | 865 |
| ws-token-keychain | 10 | pool-all | 60 | 10 | 126 | true | 865 |
