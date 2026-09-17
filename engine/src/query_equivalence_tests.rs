//! Proof that the indexed suggestion path (`Engine::suggest_with_config`) is byte-identical
//! to the reference full scan (`Engine::suggest_reference_full_scan`), and that the index's
//! candidate set is a superset of everything the reference scorer accepts.
//!
//! Query families: existing benchmark and QA inputs, every input of the human-reviewed
//! evaluation suite, a deterministically generated near-miss corpus over the production
//! packs' vocabulary (deletions, insertions, substitutions, transpositions, missing and wrong
//! diacritics, case variants, prefix truncations, one- and two-edit forms, first- and
//! last-character edits, short and long words), and Unicode/normalization edge cases.
//!
//! The real-pack suite runs in release mode as its own CI step (it is `#[ignore]` for the
//! debug run); `KURMANCI_EQUIVALENCE_SCALE=full` widens the sampling of the generated corpus
//! for a local run. Real packs are read from `data/build/packs/`; when `CI` is set they must
//! be present.

use crate::distance::weighted_damerau_levenshtein;
use crate::engine::{Engine, LexiconEntry};
use crate::normalization::{normalize, strip_diacritics};
use crate::ranking::{FrequencyMetadata, RankingConfig, Suggestion};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const ALPHABET: [char; 31] = [
    'a', 'b', 'c', 'ç', 'd', 'e', 'ê', 'f', 'g', 'h', 'i', 'î', 'j', 'k', 'l', 'm', 'n', 'o', 'p',
    'q', 'r', 's', 'ş', 't', 'u', 'û', 'v', 'w', 'x', 'y', 'z',
];

fn add_diacritic(c: char) -> Option<char> {
    match c {
        'c' => Some('ç'),
        'e' => Some('ê'),
        'i' => Some('î'),
        's' => Some('ş'),
        'u' => Some('û'),
        _ => None,
    }
}

fn remove_diacritic(c: char) -> Option<char> {
    match c {
        'ç' => Some('c'),
        'ê' => Some('e'),
        'î' => Some('i'),
        'ş' => Some('s'),
        'û' => Some('u'),
        _ => None,
    }
}

/// Deterministic generator (64-bit LCG); no randomness enters the tests.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn letter(&mut self) -> char {
        ALPHABET[(self.next() % ALPHABET.len() as u64) as usize]
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn load_pack(name: &str) -> Option<Engine> {
    let path = workspace_root().join(format!("data/build/packs/{}/lexicon.bin", name));
    match std::fs::read(&path) {
        Ok(bytes) => {
            let mut engine = Engine::new();
            engine.load_binary_pack(&bytes).unwrap();
            Some(engine)
        }
        Err(_) => {
            assert!(
                std::env::var("CI").is_err(),
                "{} must be built before the engine tests run in CI",
                path.display()
            );
            eprintln!("skipping {}: {} not built", name, path.display());
            None
        }
    }
}

fn full_scale() -> bool {
    std::env::var("KURMANCI_EQUIVALENCE_SCALE").as_deref() == Ok("full")
}

/// Near-miss variants of one word, deterministic in `rng`.
fn mutations(word: &str, rng: &mut Lcg) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    let n = chars.len();
    let mut out: Vec<String> = Vec::new();
    let mut push = |v: Vec<char>| out.push(v.into_iter().collect());
    // deletions at every position (includes first- and last-character deletion)
    for i in 0..n {
        let mut v = chars.clone();
        v.remove(i);
        push(v);
    }
    // insertions at every position, two letters each
    for i in 0..=n {
        for _ in 0..2 {
            let mut v = chars.clone();
            v.insert(i, rng.letter());
            push(v);
        }
    }
    // substitutions at every position; first and last get two more letters
    for i in 0..n {
        let extra = if i == 0 || i + 1 == n { 3 } else { 1 };
        for _ in 0..extra {
            let mut v = chars.clone();
            v[i] = rng.letter();
            push(v);
        }
    }
    // adjacent transpositions at every position
    for i in 0..n.saturating_sub(1) {
        let mut v = chars.clone();
        v.swap(i, i + 1);
        push(v);
    }
    // diacritics: all removed, each removed singly, each added singly, one wrong one
    push(
        chars
            .iter()
            .map(|c| remove_diacritic(*c).unwrap_or(*c))
            .collect(),
    );
    for i in 0..n {
        if let Some(r) = remove_diacritic(chars[i]) {
            let mut v = chars.clone();
            v[i] = r;
            push(v);
        }
        if let Some(a) = add_diacritic(chars[i]) {
            let mut v = chars.clone();
            v[i] = a;
            push(v);
        }
    }
    // case variants
    let mut title = word.to_string();
    if let Some(first) = title.chars().next() {
        title = first.to_uppercase().collect::<String>() + &word[first.len_utf8()..];
    }
    push(title.chars().collect());
    push(word.to_uppercase().chars().collect());
    // prefix truncations
    for take in [1usize, 2, n / 2, n.saturating_sub(1)] {
        if take > 0 && take < n {
            push(chars[..take].to_vec());
        }
    }
    // two-edit forms within reach of the threshold
    if n >= 3 {
        push(chars[1..n - 1].to_vec()); // delete first and last
        let mut v = chars.clone();
        v[0] = rng.letter();
        v[n - 1] = rng.letter();
        push(v); // substitute first and last
        let mut v: Vec<char> = chars
            .iter()
            .map(|c| remove_diacritic(*c).unwrap_or(*c))
            .collect();
        v.remove(n / 2);
        push(v); // stripped plus one deletion
        let mut v = chars.clone();
        v.swap(0, 1);
        v.push(rng.letter());
        push(v); // transposition plus insertion
    }
    // unchanged word itself (exact) and an unrelated insertion before the first character
    push(chars.clone());
    let mut v = chars.clone();
    v.insert(0, 'x');
    push(v);
    let mut seen = BTreeSet::new();
    out.retain(|w| !w.is_empty() && seen.insert(w.clone()));
    out
}

fn unit_osa(a: &[char], b: &[char]) -> u32 {
    let (n, m) = (a.len(), b.len());
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for (i, row) in dp.iter_mut().enumerate() {
        row[0] = i as u32;
    }
    for j in 0..=m {
        dp[0][j] = j as u32;
    }
    for i in 1..=n {
        for j in 1..=m {
            let mut v = (dp[i - 1][j - 1] + u32::from(a[i - 1] != b[j - 1]))
                .min(dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(dp[i - 2][j - 2] + 1);
            }
            dp[i][j] = v;
        }
    }
    dp[n][m]
}

/// The pruning lemma, brute-forced: whenever the weighted distance the scorer uses is within
/// its 2.0 threshold, the unit OSA distance of the diacritic-stripped forms is within 2.
#[test]
fn stripped_osa_bound_covers_every_accepted_weighted_distance() {
    let mut rng = Lcg(0x5eed_0001);
    let mut checked = 0usize;
    let mut accepted = 0usize;
    for _ in 0..40_000 {
        let n = 1 + (rng.next() % 9) as usize;
        let base: Vec<char> = (0..n).map(|_| rng.letter()).collect();
        let word: String = base.iter().collect();
        let mut variants = mutations(&word, &mut rng);
        variants.truncate(12);
        for v in variants {
            let a = normalize(&word);
            let b = normalize(&v);
            let w = weighted_damerau_levenshtein(&a, &b);
            checked += 1;
            if w <= 2.0 {
                accepted += 1;
                let sa: Vec<char> = strip_diacritics(&a).chars().collect();
                let sb: Vec<char> = strip_diacritics(&b).chars().collect();
                assert!(
                    unit_osa(&sa, &sb) <= 2,
                    "weighted {} <= 2.0 but stripped OSA {} > 2 for {:?} vs {:?}",
                    w,
                    unit_osa(&sa, &sb),
                    a,
                    b
                );
            }
        }
    }
    assert!(
        checked > 100_000 && accepted > 10_000,
        "{} / {}",
        accepted,
        checked
    );
}

fn synthetic_engine() -> Engine {
    let words = [
        "roj",
        "roja",
        "rojbaş",
        "rojbas",
        "rojba",
        "Rojbaş",
        "bijî",
        "biji",
        "baş",
        "bas",
        "baz",
        "ba",
        "b",
        "a",
        "ez",
        "ew",
        "em",
        "hûn",
        "hun",
        "tu",
        "kurd",
        "kurdî",
        "kurdi",
        "Kurdistan",
        "kurmancî",
        "kurmanci",
        "kurmanc",
        "pirtûk",
        "pirtuk",
        "pirtûkxane",
        "şev",
        "sev",
        "şevbaş",
        "çav",
        "cav",
        "çavreş",
        "welat",
        "welatparêz",
        "spas",
        "spaz",
        "heval",
        "hevall",
        "azadî",
        "azad",
        "silav",
        "silavan",
        "navê",
        "nav",
        "dil",
        "dilşad",
        "xort",
        "xortan",
        "gelek",
        "gel",
        "gelekî",
        "zimanê",
        "ziman",
        "zimanekî",
        "êdî",
        "edi",
        "îro",
        "iro",
        "ûr",
        "ur",
        "e",
        "ê",
        "ii",
        "îî",
        "abcdefghijklmnop",
        "abcdefghijklmnpo",
    ];
    let mut rng = Lcg(0x1234);
    let entries: Vec<LexiconEntry> = words
        .iter()
        .enumerate()
        .map(|(i, w)| LexiconEntry {
            word: w.to_string(),
            normalized: normalize(w),
            lemma: w.to_lowercase(),
            part_of_speech: if i % 3 == 0 { "noun" } else { "verb" }.to_string(),
            frequency: rng.next() % 1000,
            regions: vec!["general".to_string()],
            status: "approved".to_string(),
            sources: vec!["manual-seed".to_string()],
            frequency_metadata: FrequencyMetadata {
                token_count: rng.next() % 5000,
                document_count: rng.next() % 40,
                zipf_milli: (rng.next() % 6000) as u32,
            },
        })
        .collect();
    let mut engine = Engine::new();
    engine.load_lexicon(entries);
    engine
}

fn json(v: &[Suggestion]) -> String {
    serde_json::to_string(v).unwrap()
}

fn is_subset(sub: &[u32], sup: &[u32]) -> bool {
    sub.iter().all(|x| sup.binary_search(x).is_ok())
}

/// Checks one query on one engine: superset invariant, byte-identical full ranked output
/// under the given ranking config at every limit, and the public operations at limit 5.
fn check_query(engine: &Engine, query: &str, config: &RankingConfig, label: &str) {
    let accepted = engine.reference_accepted_entries(query);
    let candidates = engine.indexed_candidate_entries(query);
    assert!(
        is_subset(&accepted, &candidates),
        "[{}] index lost accepted entries for {:?}: {:?} not in candidate set",
        label,
        query,
        accepted
            .iter()
            .filter(|x| candidates.binary_search(x).is_err())
            .map(|&i| engine.lexicon.normalized(i as usize).to_string())
            .collect::<Vec<_>>()
    );
    let reference = engine.suggest_reference_full_scan(query, usize::MAX, config);
    let indexed = engine.suggest_with_config(query, usize::MAX, config);
    assert_eq!(
        json(&reference),
        json(&indexed),
        "[{}] indexed output differs from the reference for {:?}",
        label,
        query
    );
    for limit in [1usize, 5, 20] {
        let truncated: Vec<Suggestion> = reference.iter().take(limit).cloned().collect();
        assert_eq!(
            json(&truncated),
            json(&engine.suggest_with_config(query, limit, config)),
            "[{}] limit {} differs for {:?}",
            label,
            limit,
            query
        );
    }
    if config.use_frequency {
        let top5: Vec<Suggestion> = reference.iter().take(5).cloned().collect();
        let expected = json(&top5);
        assert_eq!(
            expected,
            json(&engine.suggest(query, 5)),
            "[{}] suggest {:?}",
            label,
            query
        );
        assert_eq!(
            expected,
            json(&engine.correct(query, 5)),
            "[{}] correct {:?}",
            label,
            query
        );
        assert_eq!(
            expected,
            json(&engine.complete(query, 5)),
            "[{}] complete {:?}",
            label,
            query
        );
    }
}

fn edge_case_queries() -> Vec<String> {
    [
        "",
        " ",
        "  ",
        "a",
        "ş",
        "ê",
        "x",
        "rojbaş",
        "rojbas",
        "ROJBAŞ",
        "Rojbaş",
        "rojbas\u{0327}",
        "roj\u{200B}baş",
        "\u{FEFF}rojbaş",
        "roj\u{0001}baş",
        "rojbaş\t",
        "rojba",
        "rojbaşş",
        "xyzqwv",
        "qqqqqqqq",
        "kurmancîkurmancîkurmancî",
        "abcdefghijklmnopqrstuvwxyz",
        "e\u{0302}dî",
        "bijî",
        "biji",
        "BIJÎ",
        "pirtûk",
        "pirtuk",
        "PIRTUK",
        "çav",
        "cav",
        "ÇAV",
        "12345",
        "roj baş",
        "-",
        "ro",
        "ku",
        "k",
        "kurm",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[test]
fn indexed_equals_reference_on_synthetic_lexicon() {
    let engine = synthetic_engine();
    let mut rng = Lcg(0x77);
    let mut queries: Vec<String> = edge_case_queries();
    let words: Vec<String> = engine.lexicon.words().map(str::to_string).collect();
    for w in &words {
        queries.extend(mutations(w, &mut rng));
    }
    let mut count = 0usize;
    for (i, q) in queries.iter().enumerate() {
        let config = if i % 2 == 0 {
            RankingConfig::default()
        } else {
            RankingConfig::disabled()
        };
        check_query(&engine, q, &config, "synthetic");
        if i % 2 == 0 {
            check_query(&engine, q, &RankingConfig::disabled(), "synthetic");
        }
        count += 1;
    }
    eprintln!("synthetic lexicon: {} queries checked", count);
    assert!(count > 2_000);
}

/// Precondition of the ordering argument in `finish_suggestions`: candidates are keyed by
/// normalized form and compared last by display word, so the order is total only if no two
/// entries share a display word. Every production pack satisfies this.
#[test]
fn packs_have_unique_display_words_and_normalized_forms() {
    for name in ["seed", "reviewed", "experimental-full"] {
        let Some(engine) = load_pack(name) else {
            continue;
        };
        let words: BTreeSet<&str> = engine.lexicon.words().collect();
        let normalized: BTreeSet<&str> = engine.lexicon.normalized_forms().collect();
        assert_eq!(
            words.len(),
            engine.lexicon.len(),
            "{}: duplicate display words",
            name
        );
        assert_eq!(
            normalized.len(),
            engine.lexicon.len(),
            "{}: duplicate normalized forms",
            name
        );
    }
}

fn read_jsonl_field(rel: &str, field: &str) -> Vec<String> {
    let path = workspace_root().join(rel);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v.get(field).and_then(|x| x.as_str()).map(|s| s.to_string()))
        .collect()
}

/// Heavy: the reference full scan costs ~30 ms per query on the experimental pack in release
/// mode and ten times that in debug, so this runs as its own release-mode CI step
/// ("Verify Query Index Equivalence") and is ignored in the plain debug test run:
/// `cargo test --release -p kurmanci-engine indexed_equals_reference_on_real_packs -- --ignored`
#[test]
#[ignore = "release-mode CI step; heavy in debug"]
fn indexed_equals_reference_on_real_packs() {
    let full = full_scale();
    let mut fixed: Vec<String> = Vec::new();
    // A. existing QA and benchmark queries
    fixed.extend(
        [
            "newroz",
            "peşeroj",
            "kurdis",
            "navê",
            "te",
            "rojbas",
            "spaz",
            "ro",
            "rojb",
            "welat",
            "xyzqwv",
            "ez",
            "li",
            "Kurmancî",
            "bijî",
            "pirtuk",
            "hevall",
            "kurdi",
            "rojbaş",
            "silav",
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    fixed.extend(read_jsonl_field(
        "data/benchmarks/spelling_gold.jsonl",
        "input",
    ));
    fixed.extend(read_jsonl_field(
        "data/benchmarks/completion_gold.jsonl",
        "prefix",
    ));
    // B. every input of the human-reviewed evaluation suite
    let cases = read_jsonl_field("evaluation/spelling/reviewed-cases.jsonl", "input");
    assert!(cases.len() >= 300, "evaluation cases not found");
    fixed.extend(cases);
    // E. edge cases
    fixed.extend(edge_case_queries());
    let mut seen = BTreeSet::new();
    fixed.retain(|q| seen.insert(q.clone()));

    let mut total = 0usize;
    for (name, stride) in [
        ("reviewed", if full { 1 } else { 4 }),
        ("experimental-full", if full { 100 } else { 8000 }),
    ] {
        let Some(engine) = load_pack(name) else {
            continue;
        };
        // C. generated near-miss corpus over the pack's own vocabulary, deterministic stride
        let mut rng = Lcg(0xC0FFEE);
        let mut queries = fixed.clone();
        for (i, word) in engine.lexicon.words().enumerate() {
            if i % stride == 0 {
                queries.extend(mutations(word, &mut rng));
            }
        }
        let mut count = 0usize;
        for (i, q) in queries.iter().enumerate() {
            let config = if i % 2 == 0 {
                RankingConfig::default()
            } else {
                RankingConfig::disabled()
            };
            check_query(&engine, q, &config, name);
            count += 1;
        }
        eprintln!(
            "{}: {} queries checked against the reference full scan ({} entries)",
            name,
            count,
            engine.lexicon.len()
        );
        total += count;
    }
    eprintln!("real packs: {} queries checked", total);
}

/// Stage timing of the reference pipeline on the experimental pack (release mode intended):
/// `cargo test --release -p kurmanci-engine profile_reference_stages -- --ignored --nocapture`
#[test]
#[ignore]
fn profile_reference_stages() {
    use std::time::{Duration, Instant};
    let Some(engine) = load_pack("experimental-full") else {
        return;
    };
    let queries = [
        "welat",
        "rojbas",
        "spaz",
        "ro",
        "rojb",
        "kurmancî",
        "pirtuk",
        "xyzqwv",
    ];
    let rounds = 20;
    let mut t_norm = Duration::ZERO;
    let mut t_prefix_trie = Duration::ZERO;
    let mut t_prefix_lookup = Duration::ZERO;
    let mut t_scan_strip = Duration::ZERO;
    let mut t_scan_len = Duration::ZERO;
    let mut t_scan_distance = Duration::ZERO;
    let mut t_finish = Duration::ZERO;
    let mut distance_calls = 0usize;
    let mut prefix_hits = 0usize;
    let mut t_indexed = Duration::ZERO;
    let config = RankingConfig::default();
    for _ in 0..rounds {
        for q in queries {
            let t = Instant::now();
            let norm_query = normalize(q);
            let query_stripped = strip_diacritics(&norm_query);
            t_norm += t.elapsed();

            let t = Instant::now();
            let prefix_matches = engine.trie.find_by_prefix(&norm_query);
            t_prefix_trie += t.elapsed();
            let t = Instant::now();
            for (norm_word, _) in &prefix_matches {
                std::hint::black_box(engine.lexicon.position_normalized(norm_word));
                prefix_hits += 1;
            }
            t_prefix_lookup += t.elapsed();

            let qlen = norm_query.chars().count() as isize;
            for entry_norm in engine.lexicon.normalized_forms() {
                let t = Instant::now();
                let stripped = std::hint::black_box(strip_diacritics(entry_norm));
                let diac = stripped == query_stripped;
                t_scan_strip += t.elapsed();
                if diac {
                    continue;
                }
                let t = Instant::now();
                let len_ok = (entry_norm.chars().count() as isize - qlen).abs() <= 2;
                t_scan_len += t.elapsed();
                if len_ok {
                    let t = Instant::now();
                    std::hint::black_box(weighted_damerau_levenshtein(&norm_query, entry_norm));
                    t_scan_distance += t.elapsed();
                    distance_calls += 1;
                }
            }
            let t = Instant::now();
            let _ = engine.suggest_reference_full_scan(q, 5, &config);
            let whole = t.elapsed();
            t_finish += whole;
            let t = Instant::now();
            let _ = engine.suggest_with_config(q, 5, &config);
            t_indexed += t.elapsed();
        }
    }
    let per = |d: Duration| d.as_secs_f64() * 1e6 / (rounds * queries.len()) as f64;
    eprintln!("reference stage profile, experimental-full, mean microseconds per query over {} queries x {} rounds", queries.len(), rounds);
    eprintln!("  normalize + strip query        {:>10.1}", per(t_norm));
    eprintln!(
        "  prefix: trie enumeration       {:>10.1}",
        per(t_prefix_trie)
    );
    eprintln!(
        "  prefix: linear lexicon lookups {:>10.1}  ({:.1} hits/query)",
        per(t_prefix_lookup),
        prefix_hits as f64 / (rounds * queries.len()) as f64
    );
    eprintln!(
        "  scan: strip_diacritics/entry   {:>10.1}",
        per(t_scan_strip)
    );
    eprintln!("  scan: length filter            {:>10.1}", per(t_scan_len));
    eprintln!(
        "  scan: weighted distance        {:>10.1}  ({:.0} calls/query)",
        per(t_scan_distance),
        distance_calls as f64 / (rounds * queries.len()) as f64
    );
    eprintln!("  whole reference call           {:>10.1}", per(t_finish));
    eprintln!("  whole indexed call             {:>10.1}", per(t_indexed));
}
