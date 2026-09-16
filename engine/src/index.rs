//! Query-time candidate index for `suggest` / `correct` / `complete`.
//!
//! The suggestion pipeline (`Engine::suggest_reference_full_scan`) accepts a lexicon entry `c`
//! for a normalized query `q` only when
//!
//! 1. `strip_diacritics(c) == strip_diacritics(q)` (diacritic restoration), or
//! 2. `|chars(c) - chars(q)| <= 2` and `weighted_damerau_levenshtein(q, c) <= 2.0`.
//!
//! This index produces a **superset** of those entries from a trie over the diacritic-stripped
//! forms, walked with a unit-cost optimal-string-alignment (OSA) distance bound of
//! [`MAX_STRIPPED_UNIT_DISTANCE`]. Why that cannot lose a candidate:
//!
//! Take an optimal weighted alignment of `q` and `c` with cost `W <= 2.0` and map every
//! character through `strip_diacritics`. Each transition of the alignment becomes a valid
//! unit-cost OSA transition on the stripped strings:
//!
//! - a match stays a match;
//! - a substitution of cost 0.25 is, by definition of `substitution_cost`, one of the pairs
//!   `i/î u/û s/ş c/ç e/ê`, exactly the pairs `strip_diacritics` merges, so it becomes a match
//!   (unit cost 0);
//! - a substitution of cost 0.75 or 1.0 is between characters that strip to different bases
//!   (characters stripping to the same base are precisely the 0.25 pairs), so it stays a
//!   substitution (unit cost 1);
//! - an insertion or deletion (cost 1.0) stays one (unit cost 1);
//! - an adjacent transposition (cost 0.75, requires `q[i-1] == c[j-2]` and `q[i-2] == c[j-1]`)
//!   keeps both equalities after stripping, so it stays a transposition (unit cost 1).
//!
//! Every transition that keeps unit cost 1 had weighted cost at least 0.75, so their number
//! `k` satisfies `0.75 k <= W <= 2.0`, i.e. `k <= 2`. Hence `OSA(strip(q), strip(c)) <= 2`.
//! Case 1 is distance 0. The trie walk enumerates every stored stripped form within that
//! distance exactly (an exact DP with the standard monotone pruning), so the reference's
//! accepted set is contained in the index's candidate set. The length condition of case 2 is
//! implied by the distance bound and needs no separate treatment.
//!
//! The index never decides anything: every candidate it returns is re-scored by the same code
//! the reference applies to every entry, in ascending lexicon order, so the resulting
//! candidate map and therefore the output are identical (see `Engine::suggest_indexed`).
//!
//! A second array, `by_normalized`, gives the lexicon entry of a normalized form by binary
//! search, replacing the reference's linear `find` for every trie prefix hit with the same
//! answer (the entry of smallest index carrying that normalized form).

use crate::engine::LexiconEntry;
use crate::normalization::strip_diacritics;
use crate::trie::{Trie, TrieMemory};
use std::cmp::Ordering;

/// Unit OSA radius on stripped forms that covers every weighted cost the scorer accepts.
pub const MAX_STRIPPED_UNIT_DISTANCE: u32 = 2;

#[derive(Debug, Clone, Default)]
pub(crate) struct QueryIndex {
    /// Lexicon indices ordered by `(normalized, index)`.
    by_normalized: Vec<u32>,
    /// Lexicon indices ordered by `(strip_diacritics(normalized), index)`.
    by_stripped: Vec<u32>,
    /// Trie over the distinct stripped forms; the payload of a terminal is
    /// `(start << 32) | len`, the run of `by_stripped` holding the entries with that form.
    stripped_trie: Trie,
}

/// Memory accounting of the index for `crate::memory`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueryIndexMemory {
    pub stripped_forms: usize,
    pub array_bytes: usize,
    pub array_allocations: usize,
    pub trie: TrieMemory,
}

impl QueryIndex {
    pub(crate) fn build(lexicon: &[LexiconEntry]) -> Self {
        let n = lexicon.len();
        let mut by_normalized: Vec<u32> = (0..n as u32).collect();
        by_normalized.sort_by(|&a, &b| {
            lexicon[a as usize]
                .normalized
                .cmp(&lexicon[b as usize].normalized)
                .then(a.cmp(&b))
        });

        let stripped: Vec<String> = lexicon
            .iter()
            .map(|e| strip_diacritics(&e.normalized))
            .collect();
        let mut by_stripped: Vec<u32> = (0..n as u32).collect();
        by_stripped.sort_by(|&a, &b| {
            stripped[a as usize]
                .cmp(&stripped[b as usize])
                .then(a.cmp(&b))
        });

        let mut stripped_trie = Trie::new();
        let mut start = 0usize;
        while start < n {
            let form = &stripped[by_stripped[start] as usize];
            let mut end = start + 1;
            while end < n && stripped[by_stripped[end] as usize] == *form {
                end += 1;
            }
            let payload = ((start as u64) << 32) | (end - start) as u64;
            stripped_trie.insert(form, payload);
            start = end;
        }
        stripped_trie.build();

        QueryIndex {
            by_normalized,
            by_stripped,
            stripped_trie,
        }
    }

    /// Index of the first lexicon entry whose `normalized` equals `normalized`, exactly what
    /// `lexicon.iter().position(|e| e.normalized == normalized)` returns.
    pub(crate) fn find_normalized(
        &self,
        lexicon: &[LexiconEntry],
        normalized: &str,
    ) -> Option<usize> {
        let first = self.by_normalized.partition_point(|&i| {
            lexicon[i as usize].normalized.as_str().cmp(normalized) == Ordering::Less
        });
        self.by_normalized
            .get(first)
            .map(|&i| i as usize)
            .filter(|&i| lexicon[i].normalized == normalized)
    }

    /// Ascending, duplicate-free lexicon indices of every entry whose stripped form is within
    /// unit OSA distance `max_unit_distance` of `query_stripped`.
    pub(crate) fn candidates(&self, query_stripped: &str, max_unit_distance: u32) -> Vec<u32> {
        let query: Vec<char> = query_stripped.chars().collect();
        let mut out = Vec::new();
        for payload in self
            .stripped_trie
            .fuzzy_terminals(&query, max_unit_distance)
        {
            let start = (payload >> 32) as usize;
            let len = (payload & 0xffff_ffff) as usize;
            out.extend_from_slice(&self.by_stripped[start..start + len]);
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    pub(crate) fn memory(&self) -> QueryIndexMemory {
        fn vec_bytes<T>(v: &Vec<T>) -> (usize, usize) {
            if v.capacity() == 0 {
                (0, 0)
            } else {
                (v.capacity() * std::mem::size_of::<T>(), 1)
            }
        }
        let trie = self.stripped_trie.memory();
        let mut m = QueryIndexMemory {
            stripped_forms: trie.terminal_nodes,
            trie,
            ..Default::default()
        };
        for (b, a) in [vec_bytes(&self.by_normalized), vec_bytes(&self.by_stripped)] {
            m.array_bytes += b;
            m.array_allocations += a;
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ranking::FrequencyMetadata;

    fn entry(word: &str) -> LexiconEntry {
        LexiconEntry {
            word: word.to_string(),
            normalized: crate::normalization::normalize(word),
            lemma: word.to_string(),
            part_of_speech: "noun".to_string(),
            frequency: 1,
            regions: vec![],
            status: "approved".to_string(),
            sources: vec![],
            frequency_metadata: FrequencyMetadata::default(),
        }
    }

    #[test]
    fn find_normalized_matches_linear_position() {
        let lexicon: Vec<LexiconEntry> = ["roj", "rojbaş", "bijî", "roj", "a", "şev"]
            .iter()
            .map(|w| entry(w))
            .collect();
        let index = QueryIndex::build(&lexicon);
        for probe in [
            "roj", "rojbaş", "bijî", "a", "şev", "rojba", "", "zzz", "ROJ",
        ] {
            assert_eq!(
                index.find_normalized(&lexicon, probe),
                lexicon.iter().position(|e| e.normalized == probe),
                "{}",
                probe
            );
        }
    }

    #[test]
    fn candidates_group_entries_sharing_a_stripped_form_and_are_ascending() {
        let lexicon: Vec<LexiconEntry> = ["baş", "bas", "baz", "xyz", "bâş", "ba"]
            .iter()
            .map(|w| entry(w))
            .collect();
        let index = QueryIndex::build(&lexicon);
        let c = index.candidates("bas", 0);
        assert_eq!(c, vec![0, 1]); // "baş" and "bas" strip to "bas"
        let c = index.candidates("bas", 1);
        assert_eq!(c, vec![0, 1, 2, 4, 5]); // + "baz", "bâş" (substitutions), "ba" (deletion)
        let c = index.candidates("bas", 2);
        assert!(!c.contains(&3)); // "xyz" is three substitutions away
        assert_eq!(index.memory().stripped_forms, 5);
    }
}
