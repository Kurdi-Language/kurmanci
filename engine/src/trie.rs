//! Compact completion trie.
//!
//! Nodes live in flat arrays: node `i` has label `labels[i]`, its children are the
//! contiguous range `first_child[i] .. first_child[i] + child_count[i]`, sorted by label, and
//! `terminal[i]` is the index of the word ending at the node (or `NO_WORD`). Word text is
//! stored once, concatenated, with spans and frequencies in parallel arrays. The whole trie
//! is therefore a handful of allocations instead of one hash table per node.
//!
//! Building is explicit: `insert` records words and `build` compacts them. Queries require a
//! built trie (`build` is idempotent and cheap when nothing changed). Insert order is
//! irrelevant except that inserting the same word twice keeps the last frequency, as before.
//!
//! Observable behaviour is unchanged from the previous per-node hash-map trie: `contains`
//! answers exact membership, `find_by_prefix` returns every `(word, frequency)` under a
//! prefix; the result set and values are identical, only the (previously hash-dependent)
//! enumeration order is now deterministic: depth-first, children in code point order.

use std::collections::BTreeMap;

const NO_WORD: u32 = u32::MAX;

#[derive(Debug, Clone, Default)]
pub struct Trie {
    labels: Vec<char>,
    first_child: Vec<u32>,
    child_count: Vec<u32>,
    terminal: Vec<u32>,
    /// Concatenated word text; `word_spans[k]` is `(byte offset, byte length)` of word `k`.
    word_text: String,
    word_spans: Vec<(u32, u32)>,
    word_frequency: Vec<u64>,
    /// Words recorded since the last build (last occurrence of a word wins).
    pending: Vec<(String, u64)>,
}

/// Memory accounting of a built trie for `crate::memory`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrieMemory {
    pub nodes: usize,
    pub terminal_nodes: usize,
    /// Bytes requested for the node arrays (labels, first_child, child_count, terminal).
    pub node_array_bytes: usize,
    pub node_array_allocations: usize,
    /// Bytes requested for word text, spans and frequencies.
    pub word_bytes: usize,
    pub word_allocations: usize,
    /// Bytes held by words inserted but not yet built.
    pub pending_bytes: usize,
    pub pending_allocations: usize,
}

impl Trie {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a word and its frequency. Takes effect at the next `build`.
    pub fn insert(&mut self, word: &str, frequency: u64) {
        self.pending.push((word.to_string(), frequency));
    }

    /// True when `build` has nothing to do.
    pub fn is_built(&self) -> bool {
        self.pending.is_empty()
    }

    /// Compacts all recorded words into the flat representation. Idempotent.
    pub fn build(&mut self) {
        if self.pending.is_empty() && !self.labels.is_empty() {
            return;
        }
        // Merge previously built words with the pending ones; last insert wins.
        let mut words: BTreeMap<String, u64> = BTreeMap::new();
        for k in 0..self.word_spans.len() {
            words.insert(self.word(k).to_string(), self.word_frequency[k]);
        }
        for (w, f) in self.pending.drain(..) {
            words.insert(w, f);
        }
        let sorted: Vec<(Vec<char>, String, u64)> = words
            .into_iter()
            .map(|(w, f)| (w.chars().collect(), w, f))
            .collect();

        let mut built = Trie::default();
        built
            .word_text
            .reserve(sorted.iter().map(|(_, w, _)| w.len()).sum());
        built.word_spans.reserve(sorted.len());
        built.word_frequency.reserve(sorted.len());
        for (_, w, f) in &sorted {
            built
                .word_spans
                .push((built.word_text.len() as u32, w.len() as u32));
            built.word_text.push_str(w);
            built.word_frequency.push(*f);
        }
        // Root.
        built.labels.push('\0');
        built.first_child.push(0);
        built.child_count.push(0);
        built.terminal.push(NO_WORD);
        built.build_children(&sorted, 0, sorted.len());
        built.labels.shrink_to_fit();
        built.first_child.shrink_to_fit();
        built.child_count.shrink_to_fit();
        built.terminal.shrink_to_fit();
        *self = built;
    }

    /// Creates the children of every node from the sorted words, breadth of one node at a
    /// time, using an explicit work stack instead of recursion so that build depth never
    /// depends on the call stack (a schema-valid pack may contain a word of tens of thousands
    /// of characters). Children of a node are allocated contiguously before any grandchild,
    /// which is what makes `first_child` / `child_count` ranges valid, and child tasks are
    /// pushed in reverse so they are processed first-child first: node numbering and the
    /// resulting representation are exactly those of a pre-order recursive build.
    fn build_children(&mut self, sorted: &[(Vec<char>, String, u64)], lo: usize, hi: usize) {
        // (lo, hi, depth, node): words lo..hi share the first `depth` chars and hang under `node`.
        let mut stack: Vec<(usize, usize, usize, usize)> = vec![(lo, hi, 0, 0)];
        let mut groups: Vec<(char, usize, usize)> = Vec::new();
        while let Some((lo, hi, depth, node)) = stack.pop() {
            let mut cursor = lo;
            // A word of exactly `depth` chars ends at this node (at most one: words are unique).
            if cursor < hi && sorted[cursor].0.len() == depth {
                self.terminal[node] = cursor as u32;
                cursor += 1;
            }
            // Group the remaining words by their char at `depth`; groups are contiguous
            // because the words are sorted and share the first `depth` chars.
            groups.clear();
            while cursor < hi {
                let label = sorted[cursor].0[depth];
                let start = cursor;
                while cursor < hi && sorted[cursor].0[depth] == label {
                    cursor += 1;
                }
                groups.push((label, start, cursor));
            }
            let first = self.labels.len();
            self.first_child[node] = first as u32;
            self.child_count[node] = groups.len() as u32;
            for (label, _, _) in &groups {
                self.labels.push(*label);
                self.first_child.push(0);
                self.child_count.push(0);
                self.terminal.push(NO_WORD);
            }
            for (i, (_, start, end)) in groups.iter().enumerate().rev() {
                stack.push((*start, *end, depth + 1, first + i));
            }
        }
    }

    fn word(&self, k: usize) -> &str {
        let (off, len) = self.word_spans[k];
        &self.word_text[off as usize..(off + len) as usize]
    }

    /// Node reached by walking `s` from the root, if every char has a child.
    fn walk(&self, s: &str) -> Option<usize> {
        debug_assert!(self.is_built(), "trie queried before build()");
        if self.labels.is_empty() {
            return None;
        }
        let mut node = 0usize;
        for ch in s.chars() {
            let first = self.first_child[node] as usize;
            let count = self.child_count[node] as usize;
            let children = &self.labels[first..first + count];
            let idx = children.binary_search(&ch).ok()?;
            node = first + idx;
        }
        Some(node)
    }

    /// Checks if an exact word exists in the Trie.
    pub fn contains(&self, word: &str) -> bool {
        self.walk(word)
            .map(|n| self.terminal[n] != NO_WORD)
            .unwrap_or(false)
    }

    /// Finds all words starting with the given prefix. Returns a vector of tuples
    /// `(word, frequency)` in depth-first, code point order.
    pub fn find_by_prefix(&self, prefix: &str) -> Vec<(String, u64)> {
        let Some(node) = self.walk(prefix) else {
            return Vec::new();
        };
        let mut results = Vec::new();
        self.collect_words(node, &mut results);
        results
    }

    /// Depth-first, first-child-first enumeration of the words under `node`, with an explicit
    /// stack so that traversal depth never depends on the call stack.
    fn collect_words(&self, node: usize, results: &mut Vec<(String, u64)>) {
        let mut stack: Vec<usize> = vec![node];
        while let Some(node) = stack.pop() {
            let k = self.terminal[node];
            if k != NO_WORD {
                results.push((
                    self.word(k as usize).to_string(),
                    self.word_frequency[k as usize],
                ));
            }
            let first = self.first_child[node] as usize;
            let count = self.child_count[node] as usize;
            for child in (first..first + count).rev() {
                stack.push(child);
            }
        }
    }

    /// Payloads (the value given to `insert`) of every word whose unit-cost optimal string
    /// alignment distance from `query` (insertion, deletion, substitution and adjacent
    /// transposition, each costing 1) is at most `max_dist`, in depth-first code point order.
    ///
    /// Exact: for every node a full DP row against `query` is computed from its parent's row
    /// (and its grandparent's for the transposition), and a subtree is skipped only when the
    /// minimum of the row exceeds `max_dist`. That pruning is sound for OSA because extending
    /// the candidate by one character can never lower the distance to any query prefix below
    /// the row minimum: substitution, deletion and insertion transitions add a non-negative
    /// cost to a cell of the current or previous row, and the transposition value
    /// `row[d-2][j-2] + 1` is at least `row[d-1][j-1]`, itself at least the previous row's
    /// minimum. Rows are kept per depth on an explicit stack, so neither the recursion depth
    /// nor the row storage depends on the call stack.
    pub fn fuzzy_terminals(&self, query: &[char], max_dist: u32) -> Vec<u64> {
        debug_assert!(self.is_built(), "trie queried before build()");
        let mut results = Vec::new();
        if self.labels.is_empty() {
            return results;
        }
        let m = query.len();
        let width = m + 1;
        // rows[d * width + j]: distance between the d-character path prefix and query[..j].
        let mut rows: Vec<u32> = (0..=m as u32).collect();
        let mut path: Vec<char> = Vec::new();
        if self.terminal[0] != NO_WORD && rows[m] <= max_dist {
            results.push(self.word_frequency[self.terminal[0] as usize]);
        }
        let mut stack: Vec<(u32, u32)> = Vec::new();
        let first = self.first_child[0] as usize;
        let count = self.child_count[0] as usize;
        for child in (first..first + count).rev() {
            stack.push((child as u32, 1));
        }
        while let Some((node, depth)) = stack.pop() {
            let node = node as usize;
            let d = depth as usize;
            path.truncate(d - 1);
            let c = self.labels[node];
            path.push(c);
            if rows.len() < (d + 1) * width {
                rows.resize((d + 1) * width, 0);
            }
            let (before, after) = rows.split_at_mut(d * width);
            let cur = &mut after[..width];
            let prev = &before[(d - 1) * width..];
            cur[0] = d as u32;
            let mut row_min = cur[0];
            for j in 1..=m {
                let substitution = prev[j - 1] + u32::from(c != query[j - 1]);
                let deletion = prev[j] + 1;
                let insertion = cur[j - 1] + 1;
                let mut v = substitution.min(deletion).min(insertion);
                if d >= 2 && j >= 2 && c == query[j - 2] && path[d - 2] == query[j - 1] {
                    let prev2 = &before[(d - 2) * width..(d - 1) * width];
                    v = v.min(prev2[j - 2] + 1);
                }
                cur[j] = v;
                row_min = row_min.min(v);
            }
            let k = self.terminal[node];
            if k != NO_WORD && cur[m] <= max_dist {
                results.push(self.word_frequency[k as usize]);
            }
            if row_min <= max_dist {
                let first = self.first_child[node] as usize;
                let count = self.child_count[node] as usize;
                for child in (first..first + count).rev() {
                    stack.push((child as u32, depth + 1));
                }
            }
        }
        results
    }

    /// Memory accounting for attribution (read-only).
    pub fn memory(&self) -> TrieMemory {
        fn vec_bytes<T>(v: &Vec<T>) -> (usize, usize) {
            if v.capacity() == 0 {
                (0, 0)
            } else {
                (v.capacity() * std::mem::size_of::<T>(), 1)
            }
        }
        let mut m = TrieMemory {
            nodes: self.labels.len(),
            terminal_nodes: self.terminal.iter().filter(|t| **t != NO_WORD).count(),
            ..Default::default()
        };
        for (b, a) in [
            vec_bytes(&self.labels),
            vec_bytes(&self.first_child),
            vec_bytes(&self.child_count),
            vec_bytes(&self.terminal),
        ] {
            m.node_array_bytes += b;
            m.node_array_allocations += a;
        }
        let text = if self.word_text.capacity() == 0 {
            (0, 0)
        } else {
            (self.word_text.capacity(), 1)
        };
        for (b, a) in [
            text,
            vec_bytes(&self.word_spans),
            vec_bytes(&self.word_frequency),
        ] {
            m.word_bytes += b;
            m.word_allocations += a;
        }
        let (b, a) = vec_bytes(&self.pending);
        m.pending_bytes += b;
        m.pending_allocations += a;
        for (w, _) in &self.pending {
            if w.capacity() > 0 {
                m.pending_bytes += w.capacity();
                m.pending_allocations += 1;
            }
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trie_prefix() {
        let mut trie = Trie::new();
        trie.insert("roj", 68100);
        trie.insert("roja", 72150);
        trie.insert("rojbaş", 84231);
        trie.insert("bijî", 91200);
        trie.build();

        assert!(trie.contains("rojbaş"));
        assert!(!trie.contains("rojba"));
        assert!(!trie.contains(""));

        let completions = trie.find_by_prefix("roj");
        assert_eq!(completions.len(), 3);
        assert_eq!(
            completions,
            vec![
                ("roj".to_string(), 68100),
                ("roja".to_string(), 72150),
                ("rojbaş".to_string(), 84231)
            ]
        );
        assert_eq!(trie.find_by_prefix("").len(), 4);
        assert!(trie.find_by_prefix("x").is_empty());
    }

    #[test]
    fn test_duplicate_insert_keeps_last_frequency_and_rebuild_merges() {
        let mut trie = Trie::new();
        trie.insert("roj", 1);
        trie.insert("roj", 2);
        trie.build();
        assert_eq!(trie.find_by_prefix("roj"), vec![("roj".to_string(), 2)]);
        trie.insert("roja", 3);
        trie.insert("roj", 4);
        trie.build();
        assert_eq!(
            trie.find_by_prefix("ro"),
            vec![("roj".to_string(), 4), ("roja".to_string(), 3)]
        );
        let m = trie.memory();
        assert_eq!(m.nodes, 5); // root, r, o, j, a
        assert_eq!(m.terminal_nodes, 2);
        assert_eq!(m.pending_allocations, 0);
    }

    /// Unit OSA distance, the reference for `fuzzy_terminals`.
    fn osa(a: &[char], b: &[char]) -> u32 {
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

    #[test]
    fn test_fuzzy_terminals_equal_brute_force_osa() {
        let words = [
            "",
            "a",
            "ab",
            "ba",
            "abc",
            "acb",
            "bac",
            "abcd",
            "abdc",
            "bacd",
            "xabc",
            "abcx",
            "roj",
            "rojbas",
            "rojbaz",
            "rojbsa",
            "rjobas",
            "ojbas",
            "rrojbas",
            "bijî",
            "biji",
            "şev",
            "sev",
            "çav",
            "cav",
            "kurmanci",
            "kurmancî",
            "kurmanc",
            "kurmanciy",
        ];
        let mut trie = Trie::new();
        for (k, w) in words.iter().enumerate() {
            trie.insert(w, k as u64);
        }
        trie.build();
        let queries = [
            "",
            "a",
            "b",
            "ab",
            "ba",
            "abc",
            "cba",
            "abcd",
            "rojbas",
            "rojbaş",
            "rojba",
            "ojbas",
            "rjobas",
            "rojbsa",
            "rojbasx",
            "xrojbas",
            "kurmanci",
            "kurmancî",
            "kurmnaci",
            "zzzzzz",
            "sev",
            "çav",
            "ç",
            "kurmanciyê",
            "abcdef",
        ];
        for max_dist in 0..=3u32 {
            for q in queries {
                let qc: Vec<char> = q.chars().collect();
                let mut got = trie.fuzzy_terminals(&qc, max_dist);
                got.sort_unstable();
                let mut expected: Vec<u64> = words
                    .iter()
                    .enumerate()
                    .filter(|(_, w)| {
                        let wc: Vec<char> = w.chars().collect();
                        osa(&wc, &qc) <= max_dist
                    })
                    .map(|(k, _)| k as u64)
                    .collect();
                expected.sort_unstable();
                assert_eq!(got, expected, "query {:?} max_dist {}", q, max_dist);
            }
        }
    }

    #[test]
    fn test_empty_and_unicode() {
        let mut trie = Trie::new();
        trie.build();
        assert!(!trie.contains("a"));
        assert!(trie.find_by_prefix("").is_empty());
        trie.insert("şev", 1);
        trie.insert("şevbaş", 2);
        trie.insert("çav", 3);
        trie.build();
        assert_eq!(trie.find_by_prefix("ş").len(), 2);
        assert_eq!(trie.find_by_prefix("ç"), vec![("çav".to_string(), 3)]);
        assert!(trie.contains("şevbaş"));
        assert!(!trie.contains("şevba"));
    }

    /// Build depth and enumeration depth must not depend on the call stack: a single word
    /// of 200,000 characters (far beyond any pack's 65,535-byte string limit) is built,
    /// looked up and enumerated on a thread with a 256 KiB stack, where a recursive
    /// implementation overflows within the first few thousand levels.
    #[test]
    fn test_very_deep_chain_does_not_depend_on_call_stack() {
        let handle = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let deep: String = "a".repeat(200_000);
                let sibling = format!("{}b", "a".repeat(199_999));
                let mut trie = Trie::new();
                trie.insert(&deep, 7);
                trie.insert("ab", 1);
                trie.insert(&sibling, 9);
                trie.insert(&deep, 8); // duplicate: last frequency wins
                trie.build();
                assert!(trie.contains(&deep));
                assert!(trie.contains(&sibling));
                assert!(trie.contains("ab"));
                assert!(!trie.contains(&deep[..199_999]));
                let under_prefix = trie.find_by_prefix(&"a".repeat(1000));
                assert_eq!(under_prefix.len(), 2);
                assert_eq!(under_prefix[0], (deep.clone(), 8));
                assert_eq!(under_prefix[1], (sibling.clone(), 9));
                assert_eq!(trie.find_by_prefix("").len(), 3);
                let m = trie.memory();
                assert_eq!(m.nodes, 200_003); // root + 200,000 'a' + 'b' under a¹⁹⁹⁹⁹⁹ + 'b' under a
                assert_eq!(m.terminal_nodes, 3);
            })
            .unwrap();
        handle
            .join()
            .expect("deep trie must build and answer on a small stack");
    }
}

/// The compact trie must answer exactly what the previous per-node hash-map trie answered.
/// A reference copy of that implementation lives here; both are fed the same words and
/// compared on `contains` and on the `(word, frequency)` set of `find_by_prefix` for every
/// prefix of every word plus non-matching probes. The built seed and reviewed packs, when
/// present, are checked the same way through the engine's crate-private lexicon.
#[cfg(test)]
mod equivalence_tests {
    use super::Trie;
    use std::collections::{BTreeSet, HashMap};
    use std::path::PathBuf;

    #[derive(Default)]
    struct RefNode {
        children: HashMap<char, RefNode>,
        is_terminal: bool,
        word: Option<String>,
        frequency: u64,
    }

    #[derive(Default)]
    struct RefTrie {
        root: RefNode,
    }

    impl RefTrie {
        fn insert(&mut self, word: &str, frequency: u64) {
            let mut current = &mut self.root;
            for ch in word.chars() {
                current = current.children.entry(ch).or_default();
            }
            current.is_terminal = true;
            current.word = Some(word.to_string());
            current.frequency = frequency;
        }

        fn contains(&self, word: &str) -> bool {
            let mut current = &self.root;
            for ch in word.chars() {
                match current.children.get(&ch) {
                    Some(next) => current = next,
                    None => return false,
                }
            }
            current.is_terminal
        }

        fn find_by_prefix(&self, prefix: &str) -> Vec<(String, u64)> {
            let mut current = &self.root;
            for ch in prefix.chars() {
                match current.children.get(&ch) {
                    Some(next) => current = next,
                    None => return Vec::new(),
                }
            }
            let mut results = Vec::new();
            Self::collect(current, &mut results);
            results
        }

        fn collect(node: &RefNode, results: &mut Vec<(String, u64)>) {
            if node.is_terminal {
                if let Some(word) = &node.word {
                    results.push((word.clone(), node.frequency));
                }
            }
            for child in node.children.values() {
                Self::collect(child, results);
            }
        }
    }

    fn as_set(v: Vec<(String, u64)>) -> BTreeSet<(String, u64)> {
        v.into_iter().collect()
    }

    fn probes_for(words: &[String]) -> BTreeSet<String> {
        let mut probes: BTreeSet<String> = BTreeSet::new();
        probes.insert(String::new());
        for w in words {
            let chars: Vec<char> = w.chars().collect();
            for n in 1..=chars.len() {
                probes.insert(chars[..n].iter().collect());
            }
            probes.insert(format!("{}x", w));
            probes.insert(format!("{}ê", w));
        }
        for extra in ["x", "xyz", "ş", "êê", "a b", "\u{0}", "rojbaş!", "Roj"] {
            probes.insert(extra.to_string());
        }
        probes
    }

    fn assert_equivalent(words: &[(String, u64)]) {
        let mut reference = RefTrie::default();
        let mut compact = Trie::new();
        for (w, f) in words {
            reference.insert(w, *f);
            compact.insert(w, *f);
        }
        compact.build();
        let word_list: Vec<String> = words.iter().map(|(w, _)| w.clone()).collect();
        for probe in probes_for(&word_list) {
            assert_eq!(
                compact.contains(&probe),
                reference.contains(&probe),
                "contains({:?})",
                probe
            );
            assert_eq!(
                as_set(compact.find_by_prefix(&probe)),
                as_set(reference.find_by_prefix(&probe)),
                "find_by_prefix({:?})",
                probe
            );
        }
        // The compact enumeration is deterministic: depth-first in code point order.
        let all = compact.find_by_prefix("");
        let mut sorted = all.clone();
        sorted.sort();
        assert_eq!(all, sorted);
    }

    #[test]
    fn small_vocabulary_with_duplicates_and_unicode() {
        let words: Vec<(String, u64)> = [
            ("roj", 10),
            ("roja", 20),
            ("rojbaş", 30),
            ("bijî", 40),
            ("şev", 50),
            ("şevbaş", 60),
            ("çav", 70),
            ("çawa", 80),
            ("ez", 90),
            ("e", 91),
            ("roj", 11), // duplicate: last frequency wins in both implementations
            ("a", 1),
            ("ab", 2),
            ("abc", 3),
            ("abd", 4),
            ("b", 5),
        ]
        .iter()
        .map(|(w, f)| (w.to_string(), *f))
        .collect();
        assert_equivalent(&words);
    }

    #[test]
    fn empty_and_single_word() {
        assert_equivalent(&[]);
        assert_equivalent(&[("ê".to_string(), 1)]);
    }

    /// Built packs, when present: the whole vocabulary, taken from the engine's crate-private
    /// lexicon exactly as it was inserted, through the reference and the compact trie.
    #[test]
    fn built_packs_are_equivalent_when_present() {
        for name in ["seed", "reviewed"] {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("data/build/packs")
                .join(name)
                .join("lexicon.bin");
            let Ok(bytes) = std::fs::read(&path) else {
                eprintln!("skipping {}: pack not built", name);
                continue;
            };
            let mut engine = crate::engine::Engine::new();
            engine.load_binary_pack(&bytes).unwrap();
            let words: Vec<(String, u64)> = (0..engine.lexicon.len())
                .map(|i| {
                    (
                        engine.lexicon.normalized(i).to_string(),
                        engine.lexicon.frequency(i),
                    )
                })
                .collect();
            assert!(!words.is_empty());
            assert_equivalent(&words);
            for (w, _) in words.iter().take(200) {
                assert!(engine.contains(w), "{} must be known", w);
                let first = engine.suggest(w, 1).into_iter().next().unwrap();
                assert_eq!(crate::normalization::normalize(&first.text), *w);
            }
        }
    }
}
