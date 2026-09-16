//! Read-only memory attribution for a loaded engine.
//!
//! Estimates the heap bytes owned by each engine structure from the containers' declared
//! capacities and element sizes, so that memory ownership can be explained per structure
//! (lexicon records, their strings, the completion trie, the n-gram indexes) without an
//! allocator hook. The numbers are an allocation model, not a measurement: they count what
//! the containers ask the allocator for and ignore allocator rounding and metadata, so the
//! sum is a lower bound on the process heap that the engine is responsible for. Compare it
//! with the counting-allocator figures of `kurmanci-bench` to see the gap.
//!
//! Nothing here changes engine state, ranking, prediction, or pack semantics.

use crate::engine::Engine;
use serde::Serialize;
use std::collections::HashMap;
use std::mem::size_of;

/// Bytes and counts attributed to one structure.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct StructureMemory {
    /// Estimated heap bytes requested by this structure.
    pub bytes: usize,
    /// Number of separate heap allocations the structure holds (strings, vectors, tables).
    pub allocations: usize,
}

impl StructureMemory {
    fn add(&mut self, bytes: usize, allocations: usize) {
        self.bytes += bytes;
        self.allocations += allocations;
    }
}

/// Per-structure heap attribution of a loaded engine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MemoryAttribution {
    /// Number of lexicon entries.
    pub entry_count: usize,
    /// Bytes of one `LexiconEntry` record (inline part, excluding heap payloads).
    pub entry_record_bytes: usize,
    /// `Vec<LexiconEntry>` backing storage (capacity × record size).
    pub lexicon_records: StructureMemory,
    /// Heap payloads of the five per-entry strings (word, normalized, lemma, part of speech,
    /// status).
    pub lexicon_strings: StructureMemory,
    /// Heap payloads of the per-entry `regions` and `sources` vectors and their strings.
    pub lexicon_regions_sources: StructureMemory,
    /// Total characters stored across all per-entry strings (payload length, not capacity).
    pub lexicon_string_chars: usize,
    /// Number of trie nodes including the root.
    pub trie_nodes: usize,
    /// Number of terminal trie nodes (one per distinct normalized word).
    pub trie_terminal_nodes: usize,
    /// Bytes of one trie node across the flat node arrays (label, first child, child count,
    /// terminal index).
    pub trie_node_record_bytes: usize,
    /// The flat node arrays of the compact trie.
    pub trie_node_arrays: StructureMemory,
    /// The trie's word storage: concatenated word text, spans and frequencies (plus any
    /// words inserted but not yet built).
    pub trie_words: StructureMemory,
    /// Number of bigram contexts and predictions.
    pub bigram_contexts: usize,
    pub bigram_predictions: usize,
    /// Bigram context hash table (keys and the `Vec` headers of the prediction lists).
    pub bigram_table: StructureMemory,
    /// Bigram prediction list payloads.
    pub bigram_lists: StructureMemory,
    /// Number of trigram contexts and predictions.
    pub trigram_contexts: usize,
    pub trigram_predictions: usize,
    /// Trigram context hash table.
    pub trigram_table: StructureMemory,
    /// Trigram prediction list payloads.
    pub trigram_lists: StructureMemory,
    /// Typo map (unused by packs; present for completeness).
    pub typo_map: StructureMemory,
    /// Distinct diacritic-stripped forms in the query index.
    pub query_index_stripped_forms: usize,
    /// Query index: the two sorted lexicon-index arrays.
    pub query_index_arrays: StructureMemory,
    /// Query index: node arrays of the stripped-form trie.
    pub query_index_trie_node_arrays: StructureMemory,
    /// Query index: word store of the stripped-form trie.
    pub query_index_trie_words: StructureMemory,
    /// Sum of all structures above.
    pub total: StructureMemory,
}

/// Heap bytes and allocation count of a `String` (payload capacity; zero for empty strings,
/// which do not allocate).
fn string_heap(s: &String) -> (usize, usize) {
    if s.capacity() == 0 {
        (0, 0)
    } else {
        (s.capacity(), 1)
    }
}

/// Heap bytes and allocation count of a `Vec<T>` backing store.
fn vec_heap<T>(v: &Vec<T>) -> (usize, usize) {
    if v.capacity() == 0 {
        (0, 0)
    } else {
        (v.capacity() * size_of::<T>(), 1)
    }
}

/// Heap bytes and allocation count of a `std::collections::HashMap<K, V>` table.
///
/// The standard library map is hashbrown: a table with `buckets` slots, each holding one
/// `(K, V)` pair inline, plus one control byte per bucket and a trailing group of control
/// bytes. `buckets` is the smallest power of two whose 7/8 load bound covers the capacity.
/// Small tables (capacity < 8) use 4 or 8 buckets.
fn hashmap_heap<K, V>(map: &HashMap<K, V>) -> (usize, usize) {
    let capacity = map.capacity();
    if capacity == 0 {
        return (0, 0);
    }
    let buckets = if capacity < 4 {
        4
    } else if capacity < 8 {
        8
    } else {
        // capacity_to_buckets: next power of two of ceil(capacity * 8 / 7)
        let adjusted = capacity.checked_mul(8).map(|v| v / 7).unwrap_or(usize::MAX);
        adjusted.next_power_of_two()
    };
    const GROUP_WIDTH: usize = 16;
    let bytes = buckets * size_of::<(K, V)>() + buckets + GROUP_WIDTH;
    (bytes, 1)
}

/// Computes the attribution of `engine` (read-only).
pub fn attribute(engine: &Engine) -> MemoryAttribution {
    let mut report = MemoryAttribution {
        entry_count: engine.lexicon.len(),
        entry_record_bytes: size_of::<crate::engine::LexiconEntry>(),
        trie_node_record_bytes: size_of::<char>() + 3 * size_of::<u32>(),
        ..Default::default()
    };

    // Lexicon records and their heap payloads.
    let (b, a) = vec_heap(&engine.lexicon);
    report.lexicon_records.add(b, a);
    for entry in &engine.lexicon {
        for s in [
            &entry.word,
            &entry.normalized,
            &entry.lemma,
            &entry.part_of_speech,
            &entry.status,
        ] {
            let (b, a) = string_heap(s);
            report.lexicon_strings.add(b, a);
            report.lexicon_string_chars += s.len();
        }
        for list in [&entry.regions, &entry.sources] {
            let (b, a) = vec_heap(list);
            report.lexicon_regions_sources.add(b, a);
            for s in list {
                let (b, a) = string_heap(s);
                report.lexicon_regions_sources.add(b, a);
                report.lexicon_string_chars += s.len();
            }
        }
    }

    // Trie: flat node arrays plus one concatenated word store.
    let trie = engine.trie.memory();
    report.trie_nodes = trie.nodes;
    report.trie_terminal_nodes = trie.terminal_nodes;
    report
        .trie_node_arrays
        .add(trie.node_array_bytes, trie.node_array_allocations);
    report
        .trie_words
        .add(trie.word_bytes, trie.word_allocations);
    report
        .trie_words
        .add(trie.pending_bytes, trie.pending_allocations);

    // N-gram indexes: a context table whose values are prediction vectors.
    report.bigram_contexts = engine.bigram_index.len();
    let (b, a) = hashmap_heap(&engine.bigram_index);
    report.bigram_table.add(b, a);
    for preds in engine.bigram_index.values() {
        report.bigram_predictions += preds.len();
        let (b, a) = vec_heap(preds);
        report.bigram_lists.add(b, a);
    }
    report.trigram_contexts = engine.trigram_index.len();
    let (b, a) = hashmap_heap(&engine.trigram_index);
    report.trigram_table.add(b, a);
    for preds in engine.trigram_index.values() {
        report.trigram_predictions += preds.len();
        let (b, a) = vec_heap(preds);
        report.trigram_lists.add(b, a);
    }

    // Query index: sorted index arrays plus a second compact trie over stripped forms.
    let index = engine.index.memory();
    report.query_index_stripped_forms = index.stripped_forms;
    report
        .query_index_arrays
        .add(index.array_bytes, index.array_allocations);
    report.query_index_trie_node_arrays.add(
        index.trie.node_array_bytes,
        index.trie.node_array_allocations,
    );
    report
        .query_index_trie_words
        .add(index.trie.word_bytes, index.trie.word_allocations);
    report
        .query_index_trie_words
        .add(index.trie.pending_bytes, index.trie.pending_allocations);

    let (b, a) = hashmap_heap(&engine.typo_map);
    report.typo_map.add(b, a);
    for (k, v) in &engine.typo_map {
        let (b, a) = string_heap(k);
        report.typo_map.add(b, a);
        let (b, a) = string_heap(v);
        report.typo_map.add(b, a);
    }

    for part in [
        &report.lexicon_records,
        &report.lexicon_strings,
        &report.lexicon_regions_sources,
        &report.trie_node_arrays,
        &report.trie_words,
        &report.bigram_table,
        &report.bigram_lists,
        &report.trigram_table,
        &report.trigram_lists,
        &report.typo_map,
        &report.query_index_arrays,
        &report.query_index_trie_node_arrays,
        &report.query_index_trie_words,
    ] {
        report.total.bytes += part.bytes;
        report.total.allocations += part.allocations;
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::LexiconEntry;

    fn entry(word: &str) -> LexiconEntry {
        LexiconEntry {
            word: word.to_string(),
            normalized: word.to_string(),
            lemma: word.to_string(),
            part_of_speech: "noun".to_string(),
            frequency: 0,
            regions: vec!["general".to_string()],
            status: "approved".to_string(),
            sources: vec!["manual-seed".to_string()],
            frequency_metadata: Default::default(),
        }
    }

    #[test]
    fn empty_engine_attributes_nothing() {
        let report = Engine::new().memory_attribution();
        assert_eq!(report.entry_count, 0);
        assert_eq!(report.trie_nodes, 0); // nothing built
        assert_eq!(report.total.bytes, 0);
        assert_eq!(report.total.allocations, 0);
    }

    #[test]
    fn attribution_counts_every_structure() {
        let mut engine = Engine::new();
        engine.load_lexicon(vec![entry("roj"), entry("roja"), entry("baş")]);
        let report = engine.memory_attribution();
        assert_eq!(report.entry_count, 3);
        // root + r,o,j,a + b,a,ş
        assert_eq!(report.trie_nodes, 8);
        assert_eq!(report.trie_terminal_nodes, 3);
        // One text buffer, one span array, one frequency array: three allocations for all
        // words together, and the text holds exactly the 11 bytes of "roj", "roja", "baş".
        assert_eq!(report.trie_words.allocations, 3);
        assert!(report.trie_words.bytes >= 11 + 3 * size_of::<(u32, u32)>() + 3 * size_of::<u64>());
        assert_eq!(report.trie_node_arrays.allocations, 4);
        assert_eq!(
            report.trie_node_arrays.bytes,
            8 * report.trie_node_record_bytes
        );
        // 5 strings per entry, all non-empty
        assert_eq!(report.lexicon_strings.allocations, 15);
        // word/normalized/lemma (3 each; "baş" is 4 bytes) + "noun" + "approved" per entry,
        // plus "general" + "manual-seed" per entry.
        assert_eq!(
            report.lexicon_string_chars,
            (9 + 4 + 8) + (12 + 4 + 8) + (12 + 4 + 8) + 3 * (7 + 11)
        );
        // regions and sources: one vec + one string each, per entry
        assert_eq!(report.lexicon_regions_sources.allocations, 3 * 4);
        assert_eq!(report.bigram_contexts, 0);
        assert_eq!(
            report.total.bytes,
            report.lexicon_records.bytes
                + report.lexicon_strings.bytes
                + report.lexicon_regions_sources.bytes
                + report.trie_node_arrays.bytes
                + report.trie_words.bytes
                + report.query_index_arrays.bytes
                + report.query_index_trie_node_arrays.bytes
                + report.query_index_trie_words.bytes
        );
        // Query index: two u32 arrays over 3 entries, and a stripped-form trie over
        // "roj", "roja", "bas" (baş strips to bas): root + r,o,j,a + b,a,s = 8 nodes.
        assert_eq!(report.query_index_arrays.allocations, 2);
        assert_eq!(report.query_index_arrays.bytes, 2 * 3 * size_of::<u32>());
        assert_eq!(report.query_index_stripped_forms, 3);
        assert_eq!(
            report.query_index_trie_node_arrays.bytes,
            8 * report.trie_node_record_bytes
        );
        assert!(report.total.allocations > 0);
    }

    #[test]
    fn hashmap_model_matches_capacity_rules() {
        let mut m: HashMap<usize, usize> = HashMap::new();
        assert_eq!(hashmap_heap(&m), (0, 0));
        m.insert(1, 1);
        let (bytes, allocs) = hashmap_heap(&m);
        assert_eq!(allocs, 1);
        assert!(bytes >= 4 * size_of::<(usize, usize)>() + 4 + 16);
        let big: HashMap<usize, usize> = (0..1000).map(|i| (i, i)).collect();
        let (bytes, _) = hashmap_heap(&big);
        // 1000 items need at least 1024 buckets (7/8 load bound gives 2048 at 1000 items).
        assert!(bytes >= 1024 * size_of::<(usize, usize)>());
    }
}
