# Engine memory attribution and query latency

Measured with `kurmanci-bench` (release profile) on the three authoritative packs built from
the current repository state. The purpose of this pass is attribution, not optimization: it
answers *which structure owns the memory* and *what one query costs*, so that any later
representation change targets the proven dominant structure and can be verified for identical
behaviour. No thresholds are asserted.

## Method

`kurmanci-bench` loads a pack through the public `KurmanciEngine` API under a counting
global allocator and reports:

- **engine steady-state heap**: live heap after `from_pack_bytes` returns minus the live heap
  before it (the caller-owned pack buffer is excluded);
- **peak during load** and **temporary load allocations** (peak minus steady state);
- the engine's own **per-structure attribution** (`KurmanciEngine::memory_attribution`), an
  allocation model computed from container capacities and element sizes: `Vec<LexiconEntry>`
  storage, the per-entry strings, the per-entry `regions`/`sources` vectors, the trie's
  per-node child hash tables (which store the child node records inline), the trie's
  per-terminal word copies, and the bigram/trigram context tables and prediction lists;
- process **RSS** after load and after the query loop (one engine alive); the additional cold
  loads used for the load-time distribution run last, because they briefly hold two engines
  and the allocator keeps the freed pages resident;
- **query latency** percentiles over 500 calls per operation, with the bigram, trigram and
  backoff contexts discovered from the pack itself, plus the highest transient heap one call
  allocates.

```
cargo run --release -p kurmanci-bench -- data/build/packs/reviewed/lexicon.bin
cargo run --release -p kurmanci-bench -- data/build/packs/experimental-full/lexicon.bin --json
```

Host: Apple M4, 24 GB, macOS 26.4.1, Rust 1.85.0, release profile. Numbers are from one
machine and are indicative, not a gate.

## Memory

| Pack | Entries | Pack bytes | Engine heap | Heap / pack | Bytes / entry | Allocations at load | Temporary at load | RSS after load (peak) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| seed | 33 | 3,109 | 0.04 MB | 12.7× | 1,197 | 404 | 0 | 2.2 MB |
| reviewed | 1,464 | 549,178 | 2.84 MB | 5.2× | 1,937 | 33,103 | 0 | 6.1 MB |
| experimental-full | 42,435 | 7,006,470 | 58.0 MB | 8.3× | 1,367 | 593,984 | 0 | 77.2 MB |

The attribution model explains 101% of the measured steady-state heap on every pack (the
1% is `String` capacity rounding), so the table below accounts for the whole footprint.

### Attribution, experimental-full (42,435 entries)

| Structure | MB | Share | Allocations | Count |
|---|---:|---:|---:|---|
| trie child tables | 39.06 | 66.5% | 91,383 | 123,933 nodes |
| lexicon records (`Vec<LexiconEntry>`, 200 B each) | 8.49 | 14.4% | 1 | 42,435 entries |
| lexicon regions/sources | 3.18 | 5.4% | 169,740 | 4 allocations per entry |
| trigram table | 2.69 | 4.6% | 1 | 29,516 contexts |
| lexicon strings (word, normalized, lemma, POS, status) | 1.87 | 3.2% | 212,175 | 3.0 M chars |
| trigram lists | 1.41 | 2.4% | 29,516 | 58,841 predictions |
| bigram lists | 1.15 | 1.9% | 7,594 | 47,700 predictions |
| bigram table | 0.54 | 0.9% | 1 | 7,594 contexts |
| trie word copies | 0.37 | 0.6% | 42,435 | 42,435 terminals |
| **total** | **58.74** | 100% | 552,846 | |

### Attribution, reviewed (1,464 entries)

| Structure | MB | Share | Count |
|---|---:|---:|---|
| trie child tables | 1.51 | 52.8% | 4,792 nodes |
| trigram table | 0.34 | 11.7% | 6,040 contexts |
| lexicon records | 0.29 | 10.2% | 1,464 entries |
| trigram lists | 0.27 | 9.3% | 11,093 predictions |
| bigram lists | 0.22 | 7.7% | 9,181 predictions |
| lexicon regions/sources | 0.10 | 3.6% | |
| bigram table | 0.07 | 2.4% | 1,338 contexts |
| lexicon strings | 0.06 | 1.9% | 88,953 chars |
| trie word copies | 0.01 | 0.4% | 1,464 terminals |
| **total** | **2.86** | 100% | |

## Findings

1. **The completion trie owns two thirds of the memory.** Every trie node holds its own
   `HashMap<char, TrieNode>`; with 123,933 nodes for 42,435 words that is 91,383 hash tables
   (leaves have none), each at least 4 buckets of 92 bytes plus control bytes, so a node
   costs about 315 bytes on average and the trie costs 920 bytes per word. The n-gram data
   the packs gained for prediction is 5.8 MB, one tenth of the footprint.
2. **Per-entry records are the second cost.** A `LexiconEntry` is 200 bytes inline and owns
   nine heap allocations (five strings, two vectors, two vector element strings); `status`,
   `part_of_speech`, `regions` and `sources` repeat the same few values across all entries.
   Records plus their payloads are 13.5 MB, 23% of the total, with 13 allocations per entry
   (594k allocations at load in total, which is also where the 19 MB gap between heap and
   RSS comes from: allocator metadata and rounding).
3. **Loading is already lean.** Temporary load allocations are zero on all packs: the decoder
   stages structures and moves them into place. Load time is 4 ms for reviewed and 71 ms for
   experimental-full.
4. **Earlier peak-RSS figures were a benchmark artifact.** The previously quoted 130-145 MB for
   experimental-full came from a tool that repeated cold loads while the first engine was
   alive. With one engine alive, RSS peaks at 77 MB after load and does not grow during
   queries; the net heap growth over 5,500 queries is 2.4 KB and the largest transient
   allocation of any single call is 0.14 MB.
5. **Prediction is fast; suggestion and completion scale linearly with the lexicon.** Prediction
   stays under 0.2 ms on every pack. Suggest, correct and complete cost about 1 ms on the
   reviewed pack and 24-37 ms on the experimental pack, because the suggestion pipeline
   scans every lexicon entry (diacritic stripping and edit distance per entry) and resolves
   trie hits by a linear search. This is a separate, query-time concern; it is recorded here
   because a 40k reviewed vocabulary would inherit it.

## Latency (p50 / p99, microseconds)

| Operation | Input | seed | reviewed | experimental-full |
|---|---|---:|---:|---:|
| known_hit | welat | 0.5 / 0.6 | 0.5 / 0.5 | 0.5 / 0.5 |
| known_miss | xyzqwv | 0.5 / 0.5 | 0.4 / 0.5 | 0.4 / 0.5 |
| suggest_exact | welat | 27 / 118 | 1,177 / 1,337 | 32,611 / 39,025 |
| suggest_diacritic | rojbas | 23 / 42 | 1,342 / 1,623 | 36,982 / 50,424 |
| correct_typo | spaz | 22 / 47 | 877 / 900 | 24,385 / 40,490 |
| complete_short | ro | 22 / 41 | 639 / 823 | 30,575 / 39,458 |
| complete_long | rojb | 25 / 28 | 873 / 987 | 25,279 / 28,784 |
| predict_bigram | ez | – | 0.8 / 1.0 | 22 / 105 |
| predict_backoff | xyzqwv ez | – | 2.8 / 3.0 | 90 / 134 |
| predict_trigram | (discovered) | – | 1.4 / 1.5 (ez ji) | 76 / 139 (ez ê) |
| predict_zero | xyzqwv zzqxw | 0.8 / 1.0 | 3.8 / 5.3 | 129 / 372 |

The seed pack has no n-gram data, so its prediction rows are empty by design.

## Results after the compact trie

The trie was rebuilt as flat arrays (label, first child, child count, terminal index per
node, 16 bytes; word text stored once) with children in code point order. Same release
profile and host as above.

| Pack | Engine heap before | Engine heap after | Trie before | Trie after | RSS after load before | RSS after load after | Load before | Load after |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| seed | 0.04 MB | 0.01 MB | 0.030 MB | 0.003 MB | 2.2 MB | 2.2 MB | 0.03 ms | 0.04 ms |
| reviewed | 2.84 MB | 1.45 MB | 1.52 MB | 0.11 MB | 6.1 MB | 5.1 MB | 4.2 ms | 4.7 ms |
| experimental-full | 58.0 MB | 22.4 MB | 39.4 MB | 3.0 MB | 77.2 MB | 45.8 MB | 71 ms | 75 ms |

Experimental-full attribution after: lexicon records 8.49 MB (38.0%), regions/sources 3.18 MB,
n-gram tables and lists 5.8 MB, strings 1.87 MB, trie node arrays 1.98 MB (8.9%, 123,933
nodes in 4 allocations), trie words 1.05 MB (3 allocations). Allocation calls at load went from
593,984 to 601,833 while the retained count fell from 552,846 to 419,035 (the build sorts
words through a temporary map, which is the 6.8 MB of temporary load allocation now
visible, freed before load returns). The build and the enumeration use explicit stacks,
so neither depends on the call stack for long words. Heap per entry: 1,367 to 527 bytes; heap to pack ratio
8.3× to 3.2×.

Query latency is unchanged within noise (known 0.4 µs; suggest, correct and complete still
dominated by the per-entry scan; prediction under 0.1 ms).

Equivalence evidence: the crate-internal `trie::equivalence_tests` module compares the compact trie with
a verbatim copy of the previous implementation on `contains` and the `find_by_prefix`
result set for every prefix of every word (fixtures, duplicates, Unicode, and the built seed
and reviewed packs); 756 golden queries (`known`, `suggest`, `correct`, `complete`, `predict`
over all seed words, their prefixes and probes on the reviewed and experimental-full packs)
produced byte-identical JSON before and after; the 357-case `evaluate-packs` reports are
byte-identical. The pack format is unchanged. The next dominant structure is the per-entry
lexicon record with its nine heap allocations, to be measured and decided separately.

## What this means for the next step

The dominant structure is proven: the per-node hash tables of the trie. A compact trie
representation (children in a sorted array or a flat node arena keyed by index) would remove
most of the 39 MB and most of the 91k allocations without touching what the trie answers.
Deduplicating the repeated per-entry strings and vectors is the second candidate. Both are
representation changes only; any such change must reproduce byte-identical suggestion,
completion, correction and prediction output on the engine fixtures and the 357-case
diagnostics before it merges, and must not alter ranking or pack semantics. The linear
query paths are a distinct piece of work and are not part of the memory fix.

## Results after the query-time candidate index

`suggest`, `correct` and `complete` no longer scan the lexicon. The reference pipeline is kept
verbatim as `Engine::suggest_reference_full_scan` (crate-private); the production path
`suggest_indexed` runs the same stages (`prefix_stage`, `score_entry`, `finish_suggestions`,
one copy each) over a candidate superset from `engine/src/index.rs`. Same host and profile as
above (Apple M4, release, 300 iterations per operation).

### Where the reference spent its time (experimental-full, mean per query)

| Stage | µs |
|---|---:|
| normalize + strip the query | 0.9 |
| prefix: trie enumeration | 4.7 |
| prefix: linear lexicon lookup per trie hit (23.9 hits/query) | small for these inputs; grows with the number of hits (thousands for one-letter prefixes) |
| scan: `strip_diacritics` per entry (42,435 allocations) | 24,083 |
| scan: length filter | 1,525 |
| scan: weighted edit distance (20,074 calls/query) | 18,877 |
| whole reference call | 46,979 |
| whole indexed call | 445 |

### The index

- `by_normalized`: lexicon indices sorted by normalized form; a binary search replaces the
  linear `find` for each trie prefix hit and returns the same entry (the smallest index).
- `by_stripped` and a second compact trie over the distinct diacritic-stripped forms; each
  terminal carries the run of `by_stripped` holding the entries with that form.
- A query walks the stripped trie with a unit-cost optimal-string-alignment bound of 2 and
  re-scores only the entries found, in ascending lexicon order.

Why the walk cannot drop a candidate: the scorer accepts an entry only when the stripped forms
are equal or the weighted distance is at most 2.0. In an optimal weighted alignment every
transition that is not a diacritic substitution (cost 0.25, exactly the pairs the stripping
merges) costs at least 0.75 and remains a valid unit-cost transition on the stripped strings,
so at most two of them fit under 2.0 and the stripped OSA distance is at most 2. The proof is
in `engine/src/index.rs`; `stripped_osa_bound_covers_every_accepted_weighted_distance`
brute-forces it, and `indexed_equals_reference_on_real_packs` checks on the reviewed and
experimental packs that every entry the reference accepts is in the candidate set and that the
ranked output is byte-identical (see `engine/src/query_equivalence_tests.rs`).

Ordering is unchanged because the final comparison (`RankedCandidate::cmp_with_config`) ends in
the display word and every pack has unique display words (asserted by
`packs_have_unique_display_words_and_normalized_forms`), so it is a total order that does not
depend on how or in which order candidates were found.

### Latency, p50 / p95 microseconds

| Operation | Input | reviewed before | reviewed after | experimental-full before | experimental-full after | speedup (experimental) |
|---|---|---:|---:|---:|---:|---:|
| known_hit | welat | 0.6 / 0.7 | 0.5 / 0.5 | 0.5 / 0.5 | 0.5 / 0.5 | – |
| known_miss | xyzqwv | 0.6 / 0.7 | 0.5 / 0.5 | 0.5 / 0.5 | 0.5 / 0.5 | – |
| suggest_exact | welat | 1,317 / 1,382 | 46 / 48 | 37,085 / 37,877 | 352 / 370 | 105× |
| suggest_diacritic | rojbas | 1,494 / 1,637 | 31 / 31 | 41,734 / 42,153 | 179 / 189 | 233× |
| correct_typo | spaz | 1,022 / 1,052 | 18 / 18 | 28,392 / 28,799 | 188 / 199 | 151× |
| complete_short | ro | 774 / 799 | 74 / 89 | 33,220 / 33,939 | 636 / 680 | 52× |
| complete_long | rojb | 1,021 / 1,042 | 28 / 30 | 28,593 / 29,083 | 165 / 184 | 173× |

Prediction is unchanged (it does not use the index).

### Memory and load

| Pack | Pack size | Engine heap before | Engine heap after | Query index | Trie (unchanged) | Lexicon records (unchanged) | n-gram tables (unchanged) | RSS after load before | RSS after load after | Load before | Load after |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| reviewed | 0.55 MB | 1.45 MB | 1.57 MB | 0.12 MB (arrays 0.012, trie nodes 0.073, trie words 0.033; 1,442 stripped forms) | 0.11 MB | 0.29 MB | 0.90 MB | 5.0 MB | 5.6 MB | 4.9 ms | 5.9 ms |
| experimental-full | 7.01 MB | 22.35 MB | 25.49 MB | 3.15 MB (arrays 0.34, trie nodes 1.83, trie words 0.98; 41,281 stripped forms) | 3.03 MB | 8.49 MB | 5.8 MB | 45.8 MB | 51.1 MB | 71.9 ms | 118.8 ms |

The index is a second compact trie plus two `u32` arrays: 3.15 MB on the experimental pack
(12% of the heap; the compact-trie saving of 36 MB stands). Load time grows by 47 ms on the
experimental pack, spent sorting the entries twice (by normalized and by stripped form) and
building the stripped trie; allocation calls during load rise from 601,833 to 972,261 (the
temporary stripped strings), all freed before load returns.
