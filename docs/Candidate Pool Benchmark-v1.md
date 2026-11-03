# Hybrid candidate-pool benchmark (Advisaur-Docs#548)

Recorded result of `assistant-memory-eval pool-benchmark` at code `19b801c`. The generated report follows the findings unchanged; `Candidate Pool Benchmark-v1.json` beside it holds every observation, including the page IDs of every pool, limit, case and offset. Runtimes are one release build on one Mac and are indicative only. Reproduce with:

```bash
cargo run --release -p assistant-memory-eval -- pool-benchmark --output target/pool-benchmark --code-sha "$(git rev-parse --short HEAD)"
```

## Findings

1. **Representative cases: no change.** All six score Recall@5/10 and NDCG@5/10 of 1.0 under every pool. Their filtered fused pools hold 11–13 candidates, below the smallest production pool of 15 per source, so the cap never truncates them. This set is small (54 notes), so it shows no loss on this corpus; it does not show that no corpus loses recall.
2. **The stress controls detect truncation.** A target at rank 25 of both sources is missing at limit 5 (15 per source) and ranked first with 30. A target at rank 60 is missing at both limits under the current policy and ranked second with 100. A target at rank 150 stays off the first page under every pool, because RRF ranks single-source candidates above it; pools of 200 or more only enlarge the fused total (198 → 317). Stress means: +0.40 Recall@5 at limit 5 and +0.20 Recall@10 at limit 10 for pools of 100 or more; no further gain above 100.
3. **Filters precede pooling.** The project/type control keeps its two weak decisions at ranks 1–2 under every pool, because both sources filter in SQL before the limit. The lexical/vector disagreement control is unchanged too.
4. **`total_count` reports the fused pool, not the match set.** For the kestrel query, 317 thoughts match but `total_count` is 30 at limit 5 and 59 at limit 10; pages past the fused pool are empty with `has_more: false` (offset 30 at limit 5, offset 60 at limit 10). Whether that should change is the separate API decision the spec defers.
5. **Cost.** Median handler time for the stress cases is about 1.8–1.9× the current pool at 100 per source and about 2.8× at 200 (see the tables).
6. **Parity.** Passing the production pool explicitly reproduced the unmodified handler in 66/66 calls. The `current` observations from main `2d809e4`'s handler, run on the same fixture, are byte-identical to this run's, and two runs here produced the same deterministic digest.

None of this proposes a change. A ranking or API change would be a separate item gated by these numbers.

---

## Generated report

Code: `19b801c`  
Fixture: `repository:tests/fixtures/Candidate Pool Corpus-v1.json` (candidate-pool-v1), SHA-256 `50a098b1a76c2c74198f91977acf6139d6206569c700883b02c7f03a7885b2c0`  
Documents: 449; vectors: fixture-supplied deterministic synthetic vectors; no embedding model (14 dimensions)  
Metric version: `retrieval-v1`; limits: 5,10; offsets: 0,limit,6*limit  
Pools per source: current=min(3*limit,100),pool-30=30,pool-100=100,pool-200=200,pool-all=449  
Production parity (explicit default pool vs unmodified handler): 66/66 identical  
Deterministic digest: `dc82bce7f7ec8ad2f102641d555a82819783e0a908566c0e1c8d89e2952cb428`  
Run: 2026-11-03T05:58:57Z on macos-aarch64, 346 ms

### Mean first-page metrics: representative cases

Deltas are against `current` at the same limit. Median time is per handler call.

| Limit | Policy | Pool/source | Cases | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | ΔRecall@5 | ΔRecall@10 | ΔNDCG@5 | ΔNDCG@10 | Median µs |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 5 | current | 15 | 6 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 447 |
| 5 | pool-30 | 30 | 6 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 334 |
| 5 | pool-100 | 100 | 6 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 308 |
| 5 | pool-200 | 200 | 6 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 299 |
| 5 | pool-all | 449 | 6 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 295 |
| 10 | current | 30 | 6 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 304 |
| 10 | pool-30 | 30 | 6 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 304 |
| 10 | pool-100 | 100 | 6 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 296 |
| 10 | pool-200 | 200 | 6 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 285 |
| 10 | pool-all | 449 | 6 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 291 |

### Mean first-page metrics: stress cases

Deltas are against `current` at the same limit. Median time is per handler call.

| Limit | Policy | Pool/source | Cases | Recall@5 | Recall@10 | NDCG@5 | NDCG@10 | ΔRecall@5 | ΔRecall@10 | ΔNDCG@5 | ΔNDCG@10 | Median µs |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 5 | current | 15 | 5 | 0.4000 | 0.4000 | 0.4000 | 0.4000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 662 |
| 5 | pool-30 | 30 | 5 | 0.6000 | 0.6000 | 0.6000 | 0.6000 | +0.2000 | +0.2000 | +0.2000 | +0.2000 | 685 |
| 5 | pool-100 | 100 | 5 | 0.8000 | 0.8000 | 0.7262 | 0.7262 | +0.4000 | +0.4000 | +0.3262 | +0.3262 | 1253 |
| 5 | pool-200 | 200 | 5 | 0.8000 | 0.8000 | 0.7262 | 0.7262 | +0.4000 | +0.4000 | +0.3262 | +0.3262 | 1830 |
| 5 | pool-all | 449 | 5 | 0.8000 | 0.8000 | 0.7262 | 0.7262 | +0.4000 | +0.4000 | +0.3262 | +0.3262 | 1594 |
| 10 | current | 30 | 5 | 0.6000 | 0.6000 | 0.6000 | 0.6000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 586 |
| 10 | pool-30 | 30 | 5 | 0.6000 | 0.6000 | 0.6000 | 0.6000 | +0.0000 | +0.0000 | +0.0000 | +0.0000 | 605 |
| 10 | pool-100 | 100 | 5 | 0.8000 | 0.8000 | 0.7262 | 0.7262 | +0.2000 | +0.2000 | +0.1262 | +0.1262 | 1075 |
| 10 | pool-200 | 200 | 5 | 0.8000 | 0.8000 | 0.7262 | 0.7262 | +0.2000 | +0.2000 | +0.1262 | +0.1262 | 1614 |
| 10 | pool-all | 449 | 5 | 0.8000 | 0.8000 | 0.7262 | 0.7262 | +0.2000 | +0.2000 | +0.1262 | +0.1262 | 1581 |

### First page per case

Relevant ranks are positions on the first page; `-` means not returned.

| Case | Class | Limit | Policy | Relevant ranks | Recall@10 | NDCG@10 | Fused total | Corpus |
| --- | --- | ---: | --- | --- | ---: | ---: | ---: | ---: |
| rep-bearer-token | representative | 5 | current | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 449 |
| rep-bearer-token | representative | 5 | pool-30 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 449 |
| rep-bearer-token | representative | 5 | pool-100 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 449 |
| rep-bearer-token | representative | 5 | pool-200 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 449 |
| rep-bearer-token | representative | 5 | pool-all | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 449 |
| rep-bearer-token | representative | 10 | current | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 449 |
| rep-bearer-token | representative | 10 | pool-30 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 449 |
| rep-bearer-token | representative | 10 | pool-100 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 449 |
| rep-bearer-token | representative | 10 | pool-200 | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 449 |
| rep-bearer-token | representative | 10 | pool-all | rep-t2-a1-n1:1, rep-t2-a1-n2:2 | 1.0000 | 1.0000 | 13 | 449 |
| rep-checkpoint-backup | representative | 5 | current | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-checkpoint-backup | representative | 5 | pool-30 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-checkpoint-backup | representative | 5 | pool-100 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-checkpoint-backup | representative | 5 | pool-200 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-checkpoint-backup | representative | 5 | pool-all | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-checkpoint-backup | representative | 10 | current | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-checkpoint-backup | representative | 10 | pool-30 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-checkpoint-backup | representative | 10 | pool-100 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-checkpoint-backup | representative | 10 | pool-200 | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-checkpoint-backup | representative | 10 | pool-all | rep-t0-a1-n1:1, rep-t0-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-project-alias | representative | 5 | current | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-project-alias | representative | 5 | pool-30 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-project-alias | representative | 5 | pool-100 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-project-alias | representative | 5 | pool-200 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-project-alias | representative | 5 | pool-all | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-project-alias | representative | 10 | current | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-project-alias | representative | 10 | pool-30 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-project-alias | representative | 10 | pool-100 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-project-alias | representative | 10 | pool-200 | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-project-alias | representative | 10 | pool-all | rep-t4-a1-n1:1, rep-t4-a1-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-rank-fusion | representative | 5 | current | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 449 |
| rep-rank-fusion | representative | 5 | pool-30 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 449 |
| rep-rank-fusion | representative | 5 | pool-100 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 449 |
| rep-rank-fusion | representative | 5 | pool-200 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 449 |
| rep-rank-fusion | representative | 5 | pool-all | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 449 |
| rep-rank-fusion | representative | 10 | current | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 449 |
| rep-rank-fusion | representative | 10 | pool-30 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 449 |
| rep-rank-fusion | representative | 10 | pool-100 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 449 |
| rep-rank-fusion | representative | 10 | pool-200 | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 449 |
| rep-rank-fusion | representative | 10 | pool-all | rep-t1-a1-n1:2, rep-t1-a1-n2:1 | 1.0000 | 1.0000 | 12 | 449 |
| rep-similarity-threshold | representative | 5 | current | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-similarity-threshold | representative | 5 | pool-30 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-similarity-threshold | representative | 5 | pool-100 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-similarity-threshold | representative | 5 | pool-200 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-similarity-threshold | representative | 5 | pool-all | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-similarity-threshold | representative | 10 | current | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-similarity-threshold | representative | 10 | pool-30 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-similarity-threshold | representative | 10 | pool-100 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-similarity-threshold | representative | 10 | pool-200 | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-similarity-threshold | representative | 10 | pool-all | rep-t1-a3-n1:1, rep-t1-a3-n2:2 | 1.0000 | 1.0000 | 11 | 449 |
| rep-untrusted-fence | representative | 5 | current | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 12 | 449 |
| rep-untrusted-fence | representative | 5 | pool-30 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 12 | 449 |
| rep-untrusted-fence | representative | 5 | pool-100 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 12 | 449 |
| rep-untrusted-fence | representative | 5 | pool-200 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 12 | 449 |
| rep-untrusted-fence | representative | 5 | pool-all | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 12 | 449 |
| rep-untrusted-fence | representative | 10 | current | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 12 | 449 |
| rep-untrusted-fence | representative | 10 | pool-30 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 12 | 449 |
| rep-untrusted-fence | representative | 10 | pool-100 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 12 | 449 |
| rep-untrusted-fence | representative | 10 | pool-200 | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 12 | 449 |
| rep-untrusted-fence | representative | 10 | pool-all | rep-t5-a1-n1:1, rep-t5-a1-n2:2 | 1.0000 | 1.0000 | 12 | 449 |
| stress-lexical-vector-disagreement | stress | 5 | current | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 449 |
| stress-lexical-vector-disagreement | stress | 5 | pool-30 | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 449 |
| stress-lexical-vector-disagreement | stress | 5 | pool-100 | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 449 |
| stress-lexical-vector-disagreement | stress | 5 | pool-200 | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 449 |
| stress-lexical-vector-disagreement | stress | 5 | pool-all | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 449 |
| stress-lexical-vector-disagreement | stress | 10 | current | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 449 |
| stress-lexical-vector-disagreement | stress | 10 | pool-30 | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 449 |
| stress-lexical-vector-disagreement | stress | 10 | pool-100 | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 449 |
| stress-lexical-vector-disagreement | stress | 10 | pool-200 | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 449 |
| stress-lexical-vector-disagreement | stress | 10 | pool-all | heron-lexical-1:1, heron-vector-1:2 | 1.0000 | 1.0000 | 6 | 449 |
| stress-project-type-filter | stress | 5 | current | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 449 |
| stress-project-type-filter | stress | 5 | pool-30 | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 449 |
| stress-project-type-filter | stress | 5 | pool-100 | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 449 |
| stress-project-type-filter | stress | 5 | pool-200 | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 449 |
| stress-project-type-filter | stress | 5 | pool-all | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 449 |
| stress-project-type-filter | stress | 10 | current | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 449 |
| stress-project-type-filter | stress | 10 | pool-30 | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 449 |
| stress-project-type-filter | stress | 10 | pool-100 | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 449 |
| stress-project-type-filter | stress | 10 | pool-200 | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 449 |
| stress-project-type-filter | stress | 10 | pool-all | osprey-decision-1:2, osprey-decision-2:1 | 1.0000 | 1.0000 | 2 | 449 |
| stress-tail-025 | stress | 5 | current | kestrel-tail-025:- | 0.0000 | 0.0000 | 30 | 449 |
| stress-tail-025 | stress | 5 | pool-30 | kestrel-tail-025:1 | 1.0000 | 1.0000 | 59 | 449 |
| stress-tail-025 | stress | 5 | pool-100 | kestrel-tail-025:1 | 1.0000 | 1.0000 | 198 | 449 |
| stress-tail-025 | stress | 5 | pool-200 | kestrel-tail-025:1 | 1.0000 | 1.0000 | 317 | 449 |
| stress-tail-025 | stress | 5 | pool-all | kestrel-tail-025:1 | 1.0000 | 1.0000 | 317 | 449 |
| stress-tail-025 | stress | 10 | current | kestrel-tail-025:1 | 1.0000 | 1.0000 | 59 | 449 |
| stress-tail-025 | stress | 10 | pool-30 | kestrel-tail-025:1 | 1.0000 | 1.0000 | 59 | 449 |
| stress-tail-025 | stress | 10 | pool-100 | kestrel-tail-025:1 | 1.0000 | 1.0000 | 198 | 449 |
| stress-tail-025 | stress | 10 | pool-200 | kestrel-tail-025:1 | 1.0000 | 1.0000 | 317 | 449 |
| stress-tail-025 | stress | 10 | pool-all | kestrel-tail-025:1 | 1.0000 | 1.0000 | 317 | 449 |
| stress-tail-060 | stress | 5 | current | kestrel-tail-060:- | 0.0000 | 0.0000 | 30 | 449 |
| stress-tail-060 | stress | 5 | pool-30 | kestrel-tail-060:- | 0.0000 | 0.0000 | 59 | 449 |
| stress-tail-060 | stress | 5 | pool-100 | kestrel-tail-060:2 | 1.0000 | 0.6309 | 198 | 449 |
| stress-tail-060 | stress | 5 | pool-200 | kestrel-tail-060:2 | 1.0000 | 0.6309 | 317 | 449 |
| stress-tail-060 | stress | 5 | pool-all | kestrel-tail-060:2 | 1.0000 | 0.6309 | 317 | 449 |
| stress-tail-060 | stress | 10 | current | kestrel-tail-060:- | 0.0000 | 0.0000 | 59 | 449 |
| stress-tail-060 | stress | 10 | pool-30 | kestrel-tail-060:- | 0.0000 | 0.0000 | 59 | 449 |
| stress-tail-060 | stress | 10 | pool-100 | kestrel-tail-060:2 | 1.0000 | 0.6309 | 198 | 449 |
| stress-tail-060 | stress | 10 | pool-200 | kestrel-tail-060:2 | 1.0000 | 0.6309 | 317 | 449 |
| stress-tail-060 | stress | 10 | pool-all | kestrel-tail-060:2 | 1.0000 | 0.6309 | 317 | 449 |
| stress-tail-150 | stress | 5 | current | kestrel-tail-150:- | 0.0000 | 0.0000 | 30 | 449 |
| stress-tail-150 | stress | 5 | pool-30 | kestrel-tail-150:- | 0.0000 | 0.0000 | 59 | 449 |
| stress-tail-150 | stress | 5 | pool-100 | kestrel-tail-150:- | 0.0000 | 0.0000 | 198 | 449 |
| stress-tail-150 | stress | 5 | pool-200 | kestrel-tail-150:- | 0.0000 | 0.0000 | 317 | 449 |
| stress-tail-150 | stress | 5 | pool-all | kestrel-tail-150:- | 0.0000 | 0.0000 | 317 | 449 |
| stress-tail-150 | stress | 10 | current | kestrel-tail-150:- | 0.0000 | 0.0000 | 59 | 449 |
| stress-tail-150 | stress | 10 | pool-30 | kestrel-tail-150:- | 0.0000 | 0.0000 | 59 | 449 |
| stress-tail-150 | stress | 10 | pool-100 | kestrel-tail-150:- | 0.0000 | 0.0000 | 198 | 449 |
| stress-tail-150 | stress | 10 | pool-200 | kestrel-tail-150:- | 0.0000 | 0.0000 | 317 | 449 |
| stress-tail-150 | stress | 10 | pool-all | kestrel-tail-150:- | 0.0000 | 0.0000 | 317 | 449 |

### Pagination

`total_count` is the filtered fused pool, not the corpus or the full match set.

| Case | Limit | Policy | Offset | Page size | total_count | has_more | Corpus |
| --- | ---: | --- | ---: | ---: | ---: | --- | ---: |
| rep-bearer-token | 5 | current | 0 | 5 | 13 | true | 449 |
| rep-bearer-token | 5 | current | 5 | 5 | 13 | true | 449 |
| rep-bearer-token | 5 | current | 30 | 0 | 13 | false | 449 |
| rep-bearer-token | 5 | pool-30 | 0 | 5 | 13 | true | 449 |
| rep-bearer-token | 5 | pool-30 | 5 | 5 | 13 | true | 449 |
| rep-bearer-token | 5 | pool-30 | 30 | 0 | 13 | false | 449 |
| rep-bearer-token | 5 | pool-100 | 0 | 5 | 13 | true | 449 |
| rep-bearer-token | 5 | pool-100 | 5 | 5 | 13 | true | 449 |
| rep-bearer-token | 5 | pool-100 | 30 | 0 | 13 | false | 449 |
| rep-bearer-token | 5 | pool-200 | 0 | 5 | 13 | true | 449 |
| rep-bearer-token | 5 | pool-200 | 5 | 5 | 13 | true | 449 |
| rep-bearer-token | 5 | pool-200 | 30 | 0 | 13 | false | 449 |
| rep-bearer-token | 5 | pool-all | 0 | 5 | 13 | true | 449 |
| rep-bearer-token | 5 | pool-all | 5 | 5 | 13 | true | 449 |
| rep-bearer-token | 5 | pool-all | 30 | 0 | 13 | false | 449 |
| rep-bearer-token | 10 | current | 0 | 10 | 13 | true | 449 |
| rep-bearer-token | 10 | current | 10 | 3 | 13 | false | 449 |
| rep-bearer-token | 10 | current | 60 | 0 | 13 | false | 449 |
| rep-bearer-token | 10 | pool-30 | 0 | 10 | 13 | true | 449 |
| rep-bearer-token | 10 | pool-30 | 10 | 3 | 13 | false | 449 |
| rep-bearer-token | 10 | pool-30 | 60 | 0 | 13 | false | 449 |
| rep-bearer-token | 10 | pool-100 | 0 | 10 | 13 | true | 449 |
| rep-bearer-token | 10 | pool-100 | 10 | 3 | 13 | false | 449 |
| rep-bearer-token | 10 | pool-100 | 60 | 0 | 13 | false | 449 |
| rep-bearer-token | 10 | pool-200 | 0 | 10 | 13 | true | 449 |
| rep-bearer-token | 10 | pool-200 | 10 | 3 | 13 | false | 449 |
| rep-bearer-token | 10 | pool-200 | 60 | 0 | 13 | false | 449 |
| rep-bearer-token | 10 | pool-all | 0 | 10 | 13 | true | 449 |
| rep-bearer-token | 10 | pool-all | 10 | 3 | 13 | false | 449 |
| rep-bearer-token | 10 | pool-all | 60 | 0 | 13 | false | 449 |
| rep-checkpoint-backup | 5 | current | 0 | 5 | 11 | true | 449 |
| rep-checkpoint-backup | 5 | current | 5 | 5 | 11 | true | 449 |
| rep-checkpoint-backup | 5 | current | 30 | 0 | 11 | false | 449 |
| rep-checkpoint-backup | 5 | pool-30 | 0 | 5 | 11 | true | 449 |
| rep-checkpoint-backup | 5 | pool-30 | 5 | 5 | 11 | true | 449 |
| rep-checkpoint-backup | 5 | pool-30 | 30 | 0 | 11 | false | 449 |
| rep-checkpoint-backup | 5 | pool-100 | 0 | 5 | 11 | true | 449 |
| rep-checkpoint-backup | 5 | pool-100 | 5 | 5 | 11 | true | 449 |
| rep-checkpoint-backup | 5 | pool-100 | 30 | 0 | 11 | false | 449 |
| rep-checkpoint-backup | 5 | pool-200 | 0 | 5 | 11 | true | 449 |
| rep-checkpoint-backup | 5 | pool-200 | 5 | 5 | 11 | true | 449 |
| rep-checkpoint-backup | 5 | pool-200 | 30 | 0 | 11 | false | 449 |
| rep-checkpoint-backup | 5 | pool-all | 0 | 5 | 11 | true | 449 |
| rep-checkpoint-backup | 5 | pool-all | 5 | 5 | 11 | true | 449 |
| rep-checkpoint-backup | 5 | pool-all | 30 | 0 | 11 | false | 449 |
| rep-checkpoint-backup | 10 | current | 0 | 10 | 11 | true | 449 |
| rep-checkpoint-backup | 10 | current | 10 | 1 | 11 | false | 449 |
| rep-checkpoint-backup | 10 | current | 60 | 0 | 11 | false | 449 |
| rep-checkpoint-backup | 10 | pool-30 | 0 | 10 | 11 | true | 449 |
| rep-checkpoint-backup | 10 | pool-30 | 10 | 1 | 11 | false | 449 |
| rep-checkpoint-backup | 10 | pool-30 | 60 | 0 | 11 | false | 449 |
| rep-checkpoint-backup | 10 | pool-100 | 0 | 10 | 11 | true | 449 |
| rep-checkpoint-backup | 10 | pool-100 | 10 | 1 | 11 | false | 449 |
| rep-checkpoint-backup | 10 | pool-100 | 60 | 0 | 11 | false | 449 |
| rep-checkpoint-backup | 10 | pool-200 | 0 | 10 | 11 | true | 449 |
| rep-checkpoint-backup | 10 | pool-200 | 10 | 1 | 11 | false | 449 |
| rep-checkpoint-backup | 10 | pool-200 | 60 | 0 | 11 | false | 449 |
| rep-checkpoint-backup | 10 | pool-all | 0 | 10 | 11 | true | 449 |
| rep-checkpoint-backup | 10 | pool-all | 10 | 1 | 11 | false | 449 |
| rep-checkpoint-backup | 10 | pool-all | 60 | 0 | 11 | false | 449 |
| rep-project-alias | 5 | current | 0 | 5 | 11 | true | 449 |
| rep-project-alias | 5 | current | 5 | 5 | 11 | true | 449 |
| rep-project-alias | 5 | current | 30 | 0 | 11 | false | 449 |
| rep-project-alias | 5 | pool-30 | 0 | 5 | 11 | true | 449 |
| rep-project-alias | 5 | pool-30 | 5 | 5 | 11 | true | 449 |
| rep-project-alias | 5 | pool-30 | 30 | 0 | 11 | false | 449 |
| rep-project-alias | 5 | pool-100 | 0 | 5 | 11 | true | 449 |
| rep-project-alias | 5 | pool-100 | 5 | 5 | 11 | true | 449 |
| rep-project-alias | 5 | pool-100 | 30 | 0 | 11 | false | 449 |
| rep-project-alias | 5 | pool-200 | 0 | 5 | 11 | true | 449 |
| rep-project-alias | 5 | pool-200 | 5 | 5 | 11 | true | 449 |
| rep-project-alias | 5 | pool-200 | 30 | 0 | 11 | false | 449 |
| rep-project-alias | 5 | pool-all | 0 | 5 | 11 | true | 449 |
| rep-project-alias | 5 | pool-all | 5 | 5 | 11 | true | 449 |
| rep-project-alias | 5 | pool-all | 30 | 0 | 11 | false | 449 |
| rep-project-alias | 10 | current | 0 | 10 | 11 | true | 449 |
| rep-project-alias | 10 | current | 10 | 1 | 11 | false | 449 |
| rep-project-alias | 10 | current | 60 | 0 | 11 | false | 449 |
| rep-project-alias | 10 | pool-30 | 0 | 10 | 11 | true | 449 |
| rep-project-alias | 10 | pool-30 | 10 | 1 | 11 | false | 449 |
| rep-project-alias | 10 | pool-30 | 60 | 0 | 11 | false | 449 |
| rep-project-alias | 10 | pool-100 | 0 | 10 | 11 | true | 449 |
| rep-project-alias | 10 | pool-100 | 10 | 1 | 11 | false | 449 |
| rep-project-alias | 10 | pool-100 | 60 | 0 | 11 | false | 449 |
| rep-project-alias | 10 | pool-200 | 0 | 10 | 11 | true | 449 |
| rep-project-alias | 10 | pool-200 | 10 | 1 | 11 | false | 449 |
| rep-project-alias | 10 | pool-200 | 60 | 0 | 11 | false | 449 |
| rep-project-alias | 10 | pool-all | 0 | 10 | 11 | true | 449 |
| rep-project-alias | 10 | pool-all | 10 | 1 | 11 | false | 449 |
| rep-project-alias | 10 | pool-all | 60 | 0 | 11 | false | 449 |
| rep-rank-fusion | 5 | current | 0 | 5 | 12 | true | 449 |
| rep-rank-fusion | 5 | current | 5 | 5 | 12 | true | 449 |
| rep-rank-fusion | 5 | current | 30 | 0 | 12 | false | 449 |
| rep-rank-fusion | 5 | pool-30 | 0 | 5 | 12 | true | 449 |
| rep-rank-fusion | 5 | pool-30 | 5 | 5 | 12 | true | 449 |
| rep-rank-fusion | 5 | pool-30 | 30 | 0 | 12 | false | 449 |
| rep-rank-fusion | 5 | pool-100 | 0 | 5 | 12 | true | 449 |
| rep-rank-fusion | 5 | pool-100 | 5 | 5 | 12 | true | 449 |
| rep-rank-fusion | 5 | pool-100 | 30 | 0 | 12 | false | 449 |
| rep-rank-fusion | 5 | pool-200 | 0 | 5 | 12 | true | 449 |
| rep-rank-fusion | 5 | pool-200 | 5 | 5 | 12 | true | 449 |
| rep-rank-fusion | 5 | pool-200 | 30 | 0 | 12 | false | 449 |
| rep-rank-fusion | 5 | pool-all | 0 | 5 | 12 | true | 449 |
| rep-rank-fusion | 5 | pool-all | 5 | 5 | 12 | true | 449 |
| rep-rank-fusion | 5 | pool-all | 30 | 0 | 12 | false | 449 |
| rep-rank-fusion | 10 | current | 0 | 10 | 12 | true | 449 |
| rep-rank-fusion | 10 | current | 10 | 2 | 12 | false | 449 |
| rep-rank-fusion | 10 | current | 60 | 0 | 12 | false | 449 |
| rep-rank-fusion | 10 | pool-30 | 0 | 10 | 12 | true | 449 |
| rep-rank-fusion | 10 | pool-30 | 10 | 2 | 12 | false | 449 |
| rep-rank-fusion | 10 | pool-30 | 60 | 0 | 12 | false | 449 |
| rep-rank-fusion | 10 | pool-100 | 0 | 10 | 12 | true | 449 |
| rep-rank-fusion | 10 | pool-100 | 10 | 2 | 12 | false | 449 |
| rep-rank-fusion | 10 | pool-100 | 60 | 0 | 12 | false | 449 |
| rep-rank-fusion | 10 | pool-200 | 0 | 10 | 12 | true | 449 |
| rep-rank-fusion | 10 | pool-200 | 10 | 2 | 12 | false | 449 |
| rep-rank-fusion | 10 | pool-200 | 60 | 0 | 12 | false | 449 |
| rep-rank-fusion | 10 | pool-all | 0 | 10 | 12 | true | 449 |
| rep-rank-fusion | 10 | pool-all | 10 | 2 | 12 | false | 449 |
| rep-rank-fusion | 10 | pool-all | 60 | 0 | 12 | false | 449 |
| rep-similarity-threshold | 5 | current | 0 | 5 | 11 | true | 449 |
| rep-similarity-threshold | 5 | current | 5 | 5 | 11 | true | 449 |
| rep-similarity-threshold | 5 | current | 30 | 0 | 11 | false | 449 |
| rep-similarity-threshold | 5 | pool-30 | 0 | 5 | 11 | true | 449 |
| rep-similarity-threshold | 5 | pool-30 | 5 | 5 | 11 | true | 449 |
| rep-similarity-threshold | 5 | pool-30 | 30 | 0 | 11 | false | 449 |
| rep-similarity-threshold | 5 | pool-100 | 0 | 5 | 11 | true | 449 |
| rep-similarity-threshold | 5 | pool-100 | 5 | 5 | 11 | true | 449 |
| rep-similarity-threshold | 5 | pool-100 | 30 | 0 | 11 | false | 449 |
| rep-similarity-threshold | 5 | pool-200 | 0 | 5 | 11 | true | 449 |
| rep-similarity-threshold | 5 | pool-200 | 5 | 5 | 11 | true | 449 |
| rep-similarity-threshold | 5 | pool-200 | 30 | 0 | 11 | false | 449 |
| rep-similarity-threshold | 5 | pool-all | 0 | 5 | 11 | true | 449 |
| rep-similarity-threshold | 5 | pool-all | 5 | 5 | 11 | true | 449 |
| rep-similarity-threshold | 5 | pool-all | 30 | 0 | 11 | false | 449 |
| rep-similarity-threshold | 10 | current | 0 | 10 | 11 | true | 449 |
| rep-similarity-threshold | 10 | current | 10 | 1 | 11 | false | 449 |
| rep-similarity-threshold | 10 | current | 60 | 0 | 11 | false | 449 |
| rep-similarity-threshold | 10 | pool-30 | 0 | 10 | 11 | true | 449 |
| rep-similarity-threshold | 10 | pool-30 | 10 | 1 | 11 | false | 449 |
| rep-similarity-threshold | 10 | pool-30 | 60 | 0 | 11 | false | 449 |
| rep-similarity-threshold | 10 | pool-100 | 0 | 10 | 11 | true | 449 |
| rep-similarity-threshold | 10 | pool-100 | 10 | 1 | 11 | false | 449 |
| rep-similarity-threshold | 10 | pool-100 | 60 | 0 | 11 | false | 449 |
| rep-similarity-threshold | 10 | pool-200 | 0 | 10 | 11 | true | 449 |
| rep-similarity-threshold | 10 | pool-200 | 10 | 1 | 11 | false | 449 |
| rep-similarity-threshold | 10 | pool-200 | 60 | 0 | 11 | false | 449 |
| rep-similarity-threshold | 10 | pool-all | 0 | 10 | 11 | true | 449 |
| rep-similarity-threshold | 10 | pool-all | 10 | 1 | 11 | false | 449 |
| rep-similarity-threshold | 10 | pool-all | 60 | 0 | 11 | false | 449 |
| rep-untrusted-fence | 5 | current | 0 | 5 | 12 | true | 449 |
| rep-untrusted-fence | 5 | current | 5 | 5 | 12 | true | 449 |
| rep-untrusted-fence | 5 | current | 30 | 0 | 12 | false | 449 |
| rep-untrusted-fence | 5 | pool-30 | 0 | 5 | 12 | true | 449 |
| rep-untrusted-fence | 5 | pool-30 | 5 | 5 | 12 | true | 449 |
| rep-untrusted-fence | 5 | pool-30 | 30 | 0 | 12 | false | 449 |
| rep-untrusted-fence | 5 | pool-100 | 0 | 5 | 12 | true | 449 |
| rep-untrusted-fence | 5 | pool-100 | 5 | 5 | 12 | true | 449 |
| rep-untrusted-fence | 5 | pool-100 | 30 | 0 | 12 | false | 449 |
| rep-untrusted-fence | 5 | pool-200 | 0 | 5 | 12 | true | 449 |
| rep-untrusted-fence | 5 | pool-200 | 5 | 5 | 12 | true | 449 |
| rep-untrusted-fence | 5 | pool-200 | 30 | 0 | 12 | false | 449 |
| rep-untrusted-fence | 5 | pool-all | 0 | 5 | 12 | true | 449 |
| rep-untrusted-fence | 5 | pool-all | 5 | 5 | 12 | true | 449 |
| rep-untrusted-fence | 5 | pool-all | 30 | 0 | 12 | false | 449 |
| rep-untrusted-fence | 10 | current | 0 | 10 | 12 | true | 449 |
| rep-untrusted-fence | 10 | current | 10 | 2 | 12 | false | 449 |
| rep-untrusted-fence | 10 | current | 60 | 0 | 12 | false | 449 |
| rep-untrusted-fence | 10 | pool-30 | 0 | 10 | 12 | true | 449 |
| rep-untrusted-fence | 10 | pool-30 | 10 | 2 | 12 | false | 449 |
| rep-untrusted-fence | 10 | pool-30 | 60 | 0 | 12 | false | 449 |
| rep-untrusted-fence | 10 | pool-100 | 0 | 10 | 12 | true | 449 |
| rep-untrusted-fence | 10 | pool-100 | 10 | 2 | 12 | false | 449 |
| rep-untrusted-fence | 10 | pool-100 | 60 | 0 | 12 | false | 449 |
| rep-untrusted-fence | 10 | pool-200 | 0 | 10 | 12 | true | 449 |
| rep-untrusted-fence | 10 | pool-200 | 10 | 2 | 12 | false | 449 |
| rep-untrusted-fence | 10 | pool-200 | 60 | 0 | 12 | false | 449 |
| rep-untrusted-fence | 10 | pool-all | 0 | 10 | 12 | true | 449 |
| rep-untrusted-fence | 10 | pool-all | 10 | 2 | 12 | false | 449 |
| rep-untrusted-fence | 10 | pool-all | 60 | 0 | 12 | false | 449 |
| stress-lexical-vector-disagreement | 5 | current | 0 | 5 | 6 | true | 449 |
| stress-lexical-vector-disagreement | 5 | current | 5 | 1 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 5 | current | 30 | 0 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 5 | pool-30 | 0 | 5 | 6 | true | 449 |
| stress-lexical-vector-disagreement | 5 | pool-30 | 5 | 1 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 5 | pool-30 | 30 | 0 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 5 | pool-100 | 0 | 5 | 6 | true | 449 |
| stress-lexical-vector-disagreement | 5 | pool-100 | 5 | 1 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 5 | pool-100 | 30 | 0 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 5 | pool-200 | 0 | 5 | 6 | true | 449 |
| stress-lexical-vector-disagreement | 5 | pool-200 | 5 | 1 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 5 | pool-200 | 30 | 0 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 5 | pool-all | 0 | 5 | 6 | true | 449 |
| stress-lexical-vector-disagreement | 5 | pool-all | 5 | 1 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 5 | pool-all | 30 | 0 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 10 | current | 0 | 6 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 10 | current | 10 | 0 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 10 | current | 60 | 0 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 10 | pool-30 | 0 | 6 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 10 | pool-30 | 10 | 0 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 10 | pool-30 | 60 | 0 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 10 | pool-100 | 0 | 6 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 10 | pool-100 | 10 | 0 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 10 | pool-100 | 60 | 0 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 10 | pool-200 | 0 | 6 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 10 | pool-200 | 10 | 0 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 10 | pool-200 | 60 | 0 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 10 | pool-all | 0 | 6 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 10 | pool-all | 10 | 0 | 6 | false | 449 |
| stress-lexical-vector-disagreement | 10 | pool-all | 60 | 0 | 6 | false | 449 |
| stress-project-type-filter | 5 | current | 0 | 2 | 2 | false | 449 |
| stress-project-type-filter | 5 | current | 5 | 0 | 2 | false | 449 |
| stress-project-type-filter | 5 | current | 30 | 0 | 2 | false | 449 |
| stress-project-type-filter | 5 | pool-30 | 0 | 2 | 2 | false | 449 |
| stress-project-type-filter | 5 | pool-30 | 5 | 0 | 2 | false | 449 |
| stress-project-type-filter | 5 | pool-30 | 30 | 0 | 2 | false | 449 |
| stress-project-type-filter | 5 | pool-100 | 0 | 2 | 2 | false | 449 |
| stress-project-type-filter | 5 | pool-100 | 5 | 0 | 2 | false | 449 |
| stress-project-type-filter | 5 | pool-100 | 30 | 0 | 2 | false | 449 |
| stress-project-type-filter | 5 | pool-200 | 0 | 2 | 2 | false | 449 |
| stress-project-type-filter | 5 | pool-200 | 5 | 0 | 2 | false | 449 |
| stress-project-type-filter | 5 | pool-200 | 30 | 0 | 2 | false | 449 |
| stress-project-type-filter | 5 | pool-all | 0 | 2 | 2 | false | 449 |
| stress-project-type-filter | 5 | pool-all | 5 | 0 | 2 | false | 449 |
| stress-project-type-filter | 5 | pool-all | 30 | 0 | 2 | false | 449 |
| stress-project-type-filter | 10 | current | 0 | 2 | 2 | false | 449 |
| stress-project-type-filter | 10 | current | 10 | 0 | 2 | false | 449 |
| stress-project-type-filter | 10 | current | 60 | 0 | 2 | false | 449 |
| stress-project-type-filter | 10 | pool-30 | 0 | 2 | 2 | false | 449 |
| stress-project-type-filter | 10 | pool-30 | 10 | 0 | 2 | false | 449 |
| stress-project-type-filter | 10 | pool-30 | 60 | 0 | 2 | false | 449 |
| stress-project-type-filter | 10 | pool-100 | 0 | 2 | 2 | false | 449 |
| stress-project-type-filter | 10 | pool-100 | 10 | 0 | 2 | false | 449 |
| stress-project-type-filter | 10 | pool-100 | 60 | 0 | 2 | false | 449 |
| stress-project-type-filter | 10 | pool-200 | 0 | 2 | 2 | false | 449 |
| stress-project-type-filter | 10 | pool-200 | 10 | 0 | 2 | false | 449 |
| stress-project-type-filter | 10 | pool-200 | 60 | 0 | 2 | false | 449 |
| stress-project-type-filter | 10 | pool-all | 0 | 2 | 2 | false | 449 |
| stress-project-type-filter | 10 | pool-all | 10 | 0 | 2 | false | 449 |
| stress-project-type-filter | 10 | pool-all | 60 | 0 | 2 | false | 449 |
| stress-tail-025 | 5 | current | 0 | 5 | 30 | true | 449 |
| stress-tail-025 | 5 | current | 5 | 5 | 30 | true | 449 |
| stress-tail-025 | 5 | current | 30 | 0 | 30 | false | 449 |
| stress-tail-025 | 5 | pool-30 | 0 | 5 | 59 | true | 449 |
| stress-tail-025 | 5 | pool-30 | 5 | 5 | 59 | true | 449 |
| stress-tail-025 | 5 | pool-30 | 30 | 5 | 59 | true | 449 |
| stress-tail-025 | 5 | pool-100 | 0 | 5 | 198 | true | 449 |
| stress-tail-025 | 5 | pool-100 | 5 | 5 | 198 | true | 449 |
| stress-tail-025 | 5 | pool-100 | 30 | 5 | 198 | true | 449 |
| stress-tail-025 | 5 | pool-200 | 0 | 5 | 317 | true | 449 |
| stress-tail-025 | 5 | pool-200 | 5 | 5 | 317 | true | 449 |
| stress-tail-025 | 5 | pool-200 | 30 | 5 | 317 | true | 449 |
| stress-tail-025 | 5 | pool-all | 0 | 5 | 317 | true | 449 |
| stress-tail-025 | 5 | pool-all | 5 | 5 | 317 | true | 449 |
| stress-tail-025 | 5 | pool-all | 30 | 5 | 317 | true | 449 |
| stress-tail-025 | 10 | current | 0 | 10 | 59 | true | 449 |
| stress-tail-025 | 10 | current | 10 | 10 | 59 | true | 449 |
| stress-tail-025 | 10 | current | 60 | 0 | 59 | false | 449 |
| stress-tail-025 | 10 | pool-30 | 0 | 10 | 59 | true | 449 |
| stress-tail-025 | 10 | pool-30 | 10 | 10 | 59 | true | 449 |
| stress-tail-025 | 10 | pool-30 | 60 | 0 | 59 | false | 449 |
| stress-tail-025 | 10 | pool-100 | 0 | 10 | 198 | true | 449 |
| stress-tail-025 | 10 | pool-100 | 10 | 10 | 198 | true | 449 |
| stress-tail-025 | 10 | pool-100 | 60 | 10 | 198 | true | 449 |
| stress-tail-025 | 10 | pool-200 | 0 | 10 | 317 | true | 449 |
| stress-tail-025 | 10 | pool-200 | 10 | 10 | 317 | true | 449 |
| stress-tail-025 | 10 | pool-200 | 60 | 10 | 317 | true | 449 |
| stress-tail-025 | 10 | pool-all | 0 | 10 | 317 | true | 449 |
| stress-tail-025 | 10 | pool-all | 10 | 10 | 317 | true | 449 |
| stress-tail-025 | 10 | pool-all | 60 | 10 | 317 | true | 449 |
| stress-tail-060 | 5 | current | 0 | 5 | 30 | true | 449 |
| stress-tail-060 | 5 | current | 5 | 5 | 30 | true | 449 |
| stress-tail-060 | 5 | current | 30 | 0 | 30 | false | 449 |
| stress-tail-060 | 5 | pool-30 | 0 | 5 | 59 | true | 449 |
| stress-tail-060 | 5 | pool-30 | 5 | 5 | 59 | true | 449 |
| stress-tail-060 | 5 | pool-30 | 30 | 5 | 59 | true | 449 |
| stress-tail-060 | 5 | pool-100 | 0 | 5 | 198 | true | 449 |
| stress-tail-060 | 5 | pool-100 | 5 | 5 | 198 | true | 449 |
| stress-tail-060 | 5 | pool-100 | 30 | 5 | 198 | true | 449 |
| stress-tail-060 | 5 | pool-200 | 0 | 5 | 317 | true | 449 |
| stress-tail-060 | 5 | pool-200 | 5 | 5 | 317 | true | 449 |
| stress-tail-060 | 5 | pool-200 | 30 | 5 | 317 | true | 449 |
| stress-tail-060 | 5 | pool-all | 0 | 5 | 317 | true | 449 |
| stress-tail-060 | 5 | pool-all | 5 | 5 | 317 | true | 449 |
| stress-tail-060 | 5 | pool-all | 30 | 5 | 317 | true | 449 |
| stress-tail-060 | 10 | current | 0 | 10 | 59 | true | 449 |
| stress-tail-060 | 10 | current | 10 | 10 | 59 | true | 449 |
| stress-tail-060 | 10 | current | 60 | 0 | 59 | false | 449 |
| stress-tail-060 | 10 | pool-30 | 0 | 10 | 59 | true | 449 |
| stress-tail-060 | 10 | pool-30 | 10 | 10 | 59 | true | 449 |
| stress-tail-060 | 10 | pool-30 | 60 | 0 | 59 | false | 449 |
| stress-tail-060 | 10 | pool-100 | 0 | 10 | 198 | true | 449 |
| stress-tail-060 | 10 | pool-100 | 10 | 10 | 198 | true | 449 |
| stress-tail-060 | 10 | pool-100 | 60 | 10 | 198 | true | 449 |
| stress-tail-060 | 10 | pool-200 | 0 | 10 | 317 | true | 449 |
| stress-tail-060 | 10 | pool-200 | 10 | 10 | 317 | true | 449 |
| stress-tail-060 | 10 | pool-200 | 60 | 10 | 317 | true | 449 |
| stress-tail-060 | 10 | pool-all | 0 | 10 | 317 | true | 449 |
| stress-tail-060 | 10 | pool-all | 10 | 10 | 317 | true | 449 |
| stress-tail-060 | 10 | pool-all | 60 | 10 | 317 | true | 449 |
| stress-tail-150 | 5 | current | 0 | 5 | 30 | true | 449 |
| stress-tail-150 | 5 | current | 5 | 5 | 30 | true | 449 |
| stress-tail-150 | 5 | current | 30 | 0 | 30 | false | 449 |
| stress-tail-150 | 5 | pool-30 | 0 | 5 | 59 | true | 449 |
| stress-tail-150 | 5 | pool-30 | 5 | 5 | 59 | true | 449 |
| stress-tail-150 | 5 | pool-30 | 30 | 5 | 59 | true | 449 |
| stress-tail-150 | 5 | pool-100 | 0 | 5 | 198 | true | 449 |
| stress-tail-150 | 5 | pool-100 | 5 | 5 | 198 | true | 449 |
| stress-tail-150 | 5 | pool-100 | 30 | 5 | 198 | true | 449 |
| stress-tail-150 | 5 | pool-200 | 0 | 5 | 317 | true | 449 |
| stress-tail-150 | 5 | pool-200 | 5 | 5 | 317 | true | 449 |
| stress-tail-150 | 5 | pool-200 | 30 | 5 | 317 | true | 449 |
| stress-tail-150 | 5 | pool-all | 0 | 5 | 317 | true | 449 |
| stress-tail-150 | 5 | pool-all | 5 | 5 | 317 | true | 449 |
| stress-tail-150 | 5 | pool-all | 30 | 5 | 317 | true | 449 |
| stress-tail-150 | 10 | current | 0 | 10 | 59 | true | 449 |
| stress-tail-150 | 10 | current | 10 | 10 | 59 | true | 449 |
| stress-tail-150 | 10 | current | 60 | 0 | 59 | false | 449 |
| stress-tail-150 | 10 | pool-30 | 0 | 10 | 59 | true | 449 |
| stress-tail-150 | 10 | pool-30 | 10 | 10 | 59 | true | 449 |
| stress-tail-150 | 10 | pool-30 | 60 | 0 | 59 | false | 449 |
| stress-tail-150 | 10 | pool-100 | 0 | 10 | 198 | true | 449 |
| stress-tail-150 | 10 | pool-100 | 10 | 10 | 198 | true | 449 |
| stress-tail-150 | 10 | pool-100 | 60 | 10 | 198 | true | 449 |
| stress-tail-150 | 10 | pool-200 | 0 | 10 | 317 | true | 449 |
| stress-tail-150 | 10 | pool-200 | 10 | 10 | 317 | true | 449 |
| stress-tail-150 | 10 | pool-200 | 60 | 10 | 317 | true | 449 |
| stress-tail-150 | 10 | pool-all | 0 | 10 | 317 | true | 449 |
| stress-tail-150 | 10 | pool-all | 10 | 10 | 317 | true | 449 |
| stress-tail-150 | 10 | pool-all | 60 | 10 | 317 | true | 449 |
