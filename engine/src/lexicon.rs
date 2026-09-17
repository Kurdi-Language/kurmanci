//! Compact runtime storage of the lexicon.
//!
//! A loaded pack used to become a `Vec<LexiconEntry>`: 200 bytes of record per entry plus
//! nine heap allocations (five strings, two vectors, their element strings), most of which
//! repeat the same few values (`status`, `part_of_speech`, `regions`, `sources`). This store
//! keeps the same information in a handful of flat arrays: one text arena holding every
//! `word`, `normalized` and `lemma` (a form that equals another of the same entry is stored
//! once), per-entry spans into it, parallel arrays for frequency and frequency metadata, and
//! small numeric ids into interned tables for status, part of speech, regions and sources.
//! Entry order, every field value and every query result are unchanged; the pack format is
//! untouched. `crate::memory` reports the arrays.
//!
//! Every offset and id is a `u32`. The builder converts with checked arithmetic and reports a
//! [`StoreLimitExceeded`] instead of wrapping, so a lexicon whose text, list or table sizes
//! exceed the representation fails closed: the pack loader turns it into
//! `PackLoadError::InvalidPayload` before anything reaches the engine, and `load_lexicon`
//! panics with the limit named. The store is crate-private: it is an implementation of the
//! engine's memory layout, not an integration surface.

#[cfg(test)]
use crate::engine::LexiconEntry;
use crate::ranking::FrequencyMetadata;
use std::collections::HashMap;
use std::mem::size_of;

/// A slice of the text arena.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Span {
    start: u32,
    len: u32,
}

/// A size that does not fit the store's `u32` representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoreLimitExceeded {
    /// Which quantity overflowed: text arena, region list, source list, or an intern table.
    pub(crate) what: &'static str,
    pub(crate) value: usize,
}

impl std::fmt::Display for StoreLimitExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "lexicon {} size {} exceeds the compact store's u32 representation (max {})",
            self.what,
            self.value,
            u32::MAX
        )
    }
}

fn to_u32(what: &'static str, value: usize) -> Result<u32, StoreLimitExceeded> {
    u32::try_from(value).map_err(|_| StoreLimitExceeded { what, value })
}

// Test-only hook: an artificially low text-arena limit, so the representation-limit path
// can be exercised without multi-gigabyte data. Not compiled into production builds.
#[cfg(test)]
thread_local! {
    pub(crate) static TEST_TEXT_ARENA_LIMIT: std::cell::Cell<usize> =
        const { std::cell::Cell::new(usize::MAX) };
}

fn text_arena_limit() -> usize {
    #[cfg(test)]
    {
        TEST_TEXT_ARENA_LIMIT.with(|c| c.get().min(u32::MAX as usize))
    }
    #[cfg(not(test))]
    {
        u32::MAX as usize
    }
}

/// The lexicon of a loaded engine (see the module documentation).
#[derive(Debug, Clone, Default)]
pub(crate) struct LexiconStore {
    text: String,
    word: Vec<Span>,
    normalized: Vec<Span>,
    lemma: Vec<Span>,
    frequency: Vec<u64>,
    metadata: Vec<FrequencyMetadata>,
    status: Vec<u32>,
    part_of_speech: Vec<u32>,
    /// `regions[i]` is `region_ids[region_offsets[i]..region_offsets[i + 1]]`.
    region_offsets: Vec<u32>,
    region_ids: Vec<u32>,
    source_offsets: Vec<u32>,
    source_ids: Vec<u32>,
    status_table: Vec<String>,
    part_of_speech_table: Vec<String>,
    region_table: Vec<String>,
    source_table: Vec<String>,
}

/// Memory accounting of the store for `crate::memory`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct LexiconStoreMemory {
    /// Bytes of the per-entry parallel arrays for one entry (spans, frequency, metadata, ids).
    pub(crate) record_bytes: usize,
    /// Per-entry parallel arrays (capacity × element size).
    pub(crate) record_array_bytes: usize,
    pub(crate) record_array_allocations: usize,
    /// The text arena (capacity).
    pub(crate) text_bytes: usize,
    pub(crate) text_allocations: usize,
    /// Text characters stored (payload length of the arena).
    pub(crate) text_chars: usize,
    /// Region/source id lists, their offset arrays and the four interned tables (one
    /// allocation per distinct interned value, so the count scales with the number of
    /// distinct categorical values, not with the number of entries).
    pub(crate) tables_bytes: usize,
    pub(crate) tables_allocations: usize,
    /// Characters held by the interned tables.
    pub(crate) table_chars: usize,
}

fn vec_bytes<T>(v: &Vec<T>) -> (usize, usize) {
    if v.capacity() == 0 {
        (0, 0)
    } else {
        (v.capacity() * size_of::<T>(), 1)
    }
}

fn string_bytes(s: &String) -> (usize, usize) {
    if s.capacity() == 0 {
        (0, 0)
    } else {
        (s.capacity(), 1)
    }
}

/// Builds a `LexiconStore` one entry at a time, interning the repeated values.
#[derive(Debug, Default)]
pub(crate) struct LexiconStoreBuilder {
    store: LexiconStore,
    status_ids: HashMap<String, u32>,
    part_of_speech_ids: HashMap<String, u32>,
    region_ids: HashMap<String, u32>,
    source_ids: HashMap<String, u32>,
}

fn intern(
    what: &'static str,
    table: &mut Vec<String>,
    ids: &mut HashMap<String, u32>,
    value: &str,
) -> Result<u32, StoreLimitExceeded> {
    if let Some(&id) = ids.get(value) {
        return Ok(id);
    }
    let id = to_u32(what, table.len())?;
    table.push(value.to_string());
    ids.insert(value.to_string(), id);
    Ok(id)
}

impl LexiconStoreBuilder {
    /// Pre-sizes the arrays for `count` entries.
    pub(crate) fn with_capacity(count: usize) -> Self {
        let mut b = Self::default();
        let s = &mut b.store;
        s.word.reserve(count);
        s.normalized.reserve(count);
        s.lemma.reserve(count);
        s.frequency.reserve(count);
        s.metadata.reserve(count);
        s.status.reserve(count);
        s.part_of_speech.reserve(count);
        s.region_offsets.reserve(count + 1);
        s.source_offsets.reserve(count + 1);
        s.region_offsets.push(0);
        s.source_offsets.push(0);
        b
    }

    /// Appends `value` to the arena. The span is checked before the text is written, so a
    /// failed push leaves the arena unchanged.
    fn push_text(&mut self, value: &str) -> Result<Span, StoreLimitExceeded> {
        let start = to_u32("text arena", self.store.text.len())?;
        let len = to_u32("text arena", value.len())?;
        let end = self.store.text.len().saturating_add(value.len());
        if end > text_arena_limit() {
            return Err(StoreLimitExceeded {
                what: "text arena",
                value: end,
            });
        }
        self.store.text.push_str(value);
        Ok(Span { start, len })
    }

    /// Appends one entry. Fields are stored exactly as given. Fails closed when a text
    /// offset, list offset or intern id would not fit the `u32` representation; the caller
    /// must then discard the builder (the pack loader discards its staged state, so the
    /// engine is untouched).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push<'a>(
        &mut self,
        word: &str,
        normalized: &str,
        lemma: &str,
        part_of_speech: &str,
        frequency: u64,
        status: &str,
        regions: impl IntoIterator<Item = &'a str>,
        sources: impl IntoIterator<Item = &'a str>,
        metadata: FrequencyMetadata,
    ) -> Result<(), StoreLimitExceeded> {
        if self.store.region_offsets.is_empty() {
            self.store.region_offsets.push(0);
            self.store.source_offsets.push(0);
        }
        let word_span = self.push_text(word)?;
        let normalized_span = if normalized == word {
            word_span
        } else {
            self.push_text(normalized)?
        };
        let lemma_span = if lemma == word {
            word_span
        } else if lemma == normalized {
            normalized_span
        } else {
            self.push_text(lemma)?
        };
        let status_id = intern(
            "status table",
            &mut self.store.status_table,
            &mut self.status_ids,
            status,
        )?;
        let pos_id = intern(
            "part-of-speech table",
            &mut self.store.part_of_speech_table,
            &mut self.part_of_speech_ids,
            part_of_speech,
        )?;
        for r in regions {
            let id = intern(
                "region table",
                &mut self.store.region_table,
                &mut self.region_ids,
                r,
            )?;
            self.store.region_ids.push(id);
        }
        let region_end = to_u32("region list", self.store.region_ids.len())?;
        self.store.region_offsets.push(region_end);
        for src in sources {
            let id = intern(
                "source table",
                &mut self.store.source_table,
                &mut self.source_ids,
                src,
            )?;
            self.store.source_ids.push(id);
        }
        let source_end = to_u32("source list", self.store.source_ids.len())?;
        self.store.source_offsets.push(source_end);

        let s = &mut self.store;
        s.word.push(word_span);
        s.normalized.push(normalized_span);
        s.lemma.push(lemma_span);
        s.frequency.push(frequency);
        s.metadata.push(metadata);
        s.status.push(status_id);
        s.part_of_speech.push(pos_id);
        Ok(())
    }

    /// Finishes the store, releasing spare capacity.
    pub(crate) fn finish(mut self) -> LexiconStore {
        let s = &mut self.store;
        if s.region_offsets.is_empty() {
            s.region_offsets.push(0);
            s.source_offsets.push(0);
        }
        s.text.shrink_to_fit();
        s.word.shrink_to_fit();
        s.normalized.shrink_to_fit();
        s.lemma.shrink_to_fit();
        s.frequency.shrink_to_fit();
        s.metadata.shrink_to_fit();
        s.status.shrink_to_fit();
        s.part_of_speech.shrink_to_fit();
        s.region_offsets.shrink_to_fit();
        s.region_ids.shrink_to_fit();
        s.source_offsets.shrink_to_fit();
        s.source_ids.shrink_to_fit();
        s.status_table.shrink_to_fit();
        s.part_of_speech_table.shrink_to_fit();
        s.region_table.shrink_to_fit();
        s.source_table.shrink_to_fit();
        self.store
    }
}

impl LexiconStore {
    /// Builds a store from owned entries, in order (test support).
    #[cfg(test)]
    pub(crate) fn from_entries(entries: Vec<LexiconEntry>) -> Result<Self, StoreLimitExceeded> {
        let mut b = LexiconStoreBuilder::with_capacity(entries.len());
        for e in &entries {
            b.push(
                &e.word,
                &e.normalized,
                &e.lemma,
                &e.part_of_speech,
                e.frequency,
                &e.status,
                e.regions.iter().map(String::as_str),
                e.sources.iter().map(String::as_str),
                e.frequency_metadata,
            )?;
        }
        Ok(b.finish())
    }

    pub(crate) fn len(&self) -> usize {
        self.word.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.word.is_empty()
    }

    fn text(&self, span: Span) -> &str {
        let start = span.start as usize;
        &self.text[start..start + span.len as usize]
    }

    pub(crate) fn word(&self, i: usize) -> &str {
        self.text(self.word[i])
    }

    pub(crate) fn normalized(&self, i: usize) -> &str {
        self.text(self.normalized[i])
    }

    pub(crate) fn lemma(&self, i: usize) -> &str {
        self.text(self.lemma[i])
    }

    pub(crate) fn part_of_speech(&self, i: usize) -> &str {
        &self.part_of_speech_table[self.part_of_speech[i] as usize]
    }

    pub(crate) fn status(&self, i: usize) -> &str {
        &self.status_table[self.status[i] as usize]
    }

    pub(crate) fn frequency(&self, i: usize) -> u64 {
        self.frequency[i]
    }

    pub(crate) fn frequency_metadata(&self, i: usize) -> FrequencyMetadata {
        self.metadata[i]
    }

    pub(crate) fn regions(&self, i: usize) -> impl Iterator<Item = &str> {
        let (a, b) = (
            self.region_offsets[i] as usize,
            self.region_offsets[i + 1] as usize,
        );
        self.region_ids[a..b]
            .iter()
            .map(move |&id| self.region_table[id as usize].as_str())
    }

    pub(crate) fn sources(&self, i: usize) -> impl Iterator<Item = &str> {
        let (a, b) = (
            self.source_offsets[i] as usize,
            self.source_offsets[i + 1] as usize,
        );
        self.source_ids[a..b]
            .iter()
            .map(move |&id| self.source_table[id as usize].as_str())
    }

    /// The entry materialized as the public record type (test support only).
    #[cfg(test)]
    pub(crate) fn entry(&self, i: usize) -> LexiconEntry {
        LexiconEntry {
            word: self.word(i).to_string(),
            normalized: self.normalized(i).to_string(),
            lemma: self.lemma(i).to_string(),
            part_of_speech: self.part_of_speech(i).to_string(),
            frequency: self.frequency(i),
            regions: self.regions(i).map(str::to_string).collect(),
            status: self.status(i).to_string(),
            sources: self.sources(i).map(str::to_string).collect(),
            frequency_metadata: self.frequency_metadata(i),
        }
    }

    /// Index of the first entry whose normalized form equals `normalized`, by linear scan
    /// (the reference semantics; the query index answers the same question by binary search).
    pub(crate) fn position_normalized(&self, normalized: &str) -> Option<usize> {
        (0..self.len()).find(|&i| self.normalized(i) == normalized)
    }

    /// Every normalized form in entry order.
    pub(crate) fn normalized_forms(&self) -> impl Iterator<Item = &str> {
        (0..self.len()).map(move |i| self.normalized(i))
    }

    /// Every display word in entry order (test support).
    #[cfg(test)]
    pub(crate) fn words(&self) -> impl Iterator<Item = &str> {
        (0..self.len()).map(move |i| self.word(i))
    }

    /// Memory accounting (read-only).
    pub(crate) fn memory(&self) -> LexiconStoreMemory {
        let mut m = LexiconStoreMemory {
            record_bytes: 3 * size_of::<Span>()
                + size_of::<u64>()
                + size_of::<FrequencyMetadata>()
                + 2 * size_of::<u32>()
                + 2 * size_of::<u32>(),
            ..Default::default()
        };
        for (b, a) in [
            vec_bytes(&self.word),
            vec_bytes(&self.normalized),
            vec_bytes(&self.lemma),
            vec_bytes(&self.frequency),
            vec_bytes(&self.metadata),
            vec_bytes(&self.status),
            vec_bytes(&self.part_of_speech),
            vec_bytes(&self.region_offsets),
            vec_bytes(&self.source_offsets),
        ] {
            m.record_array_bytes += b;
            m.record_array_allocations += a;
        }
        let (b, a) = string_bytes(&self.text);
        m.text_bytes = b;
        m.text_allocations = a;
        m.text_chars = self.text.len();
        for (b, a) in [vec_bytes(&self.region_ids), vec_bytes(&self.source_ids)] {
            m.tables_bytes += b;
            m.tables_allocations += a;
        }
        for table in [
            &self.status_table,
            &self.part_of_speech_table,
            &self.region_table,
            &self.source_table,
        ] {
            let (b, a) = vec_bytes(table);
            m.tables_bytes += b;
            m.tables_allocations += a;
            for s in table {
                let (b, a) = string_bytes(s);
                m.tables_bytes += b;
                m.tables_allocations += a;
                m.table_chars += s.len();
            }
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(word: &str, normalized: &str, lemma: &str, status: &str) -> LexiconEntry {
        LexiconEntry {
            word: word.to_string(),
            normalized: normalized.to_string(),
            lemma: lemma.to_string(),
            part_of_speech: "noun".to_string(),
            frequency: 7,
            regions: vec!["general".to_string(), "north".to_string()],
            status: status.to_string(),
            sources: vec!["manual-seed".to_string()],
            frequency_metadata: FrequencyMetadata {
                token_count: 1,
                document_count: 2,
                zipf_milli: 3,
            },
        }
    }

    #[test]
    fn round_trips_every_field_in_order() {
        let entries = vec![
            entry("Rojbaş", "rojbaş", "rojbaş", "seed"),
            entry("şev", "şev", "şev", "approved"),
            entry("bijî", "bijî", "bijîn", "seed"),
            LexiconEntry {
                regions: vec![],
                sources: vec![],
                ..entry("a", "a", "a", "seed")
            },
        ];
        let store = LexiconStore::from_entries(entries.clone()).unwrap();
        assert_eq!(store.len(), entries.len());
        for (i, e) in entries.iter().enumerate() {
            let back = store.entry(i);
            assert_eq!(back.word, e.word);
            assert_eq!(back.normalized, e.normalized);
            assert_eq!(back.lemma, e.lemma);
            assert_eq!(back.part_of_speech, e.part_of_speech);
            assert_eq!(back.frequency, e.frequency);
            assert_eq!(back.regions, e.regions);
            assert_eq!(back.status, e.status);
            assert_eq!(back.sources, e.sources);
            assert_eq!(back.frequency_metadata, e.frequency_metadata);
        }
        assert_eq!(store.position_normalized("bijî"), Some(2));
        assert_eq!(store.position_normalized("xyz"), None);
        // Shared forms are stored once: "şev" ×3 and "a" ×3 occupy one slice each.
        assert_eq!(store.text, "Rojbaşrojbaşşevbijîbijîna");
        assert_eq!(store.status_table, vec!["seed", "approved"]);
        assert_eq!(store.region_table, vec!["general", "north"]);
        let m = store.memory();
        assert_eq!(m.record_bytes, 72);
        assert_eq!(m.text_chars, store.text.len());
    }

    #[test]
    fn limits_fail_closed_without_wrapping() {
        assert_eq!(
            to_u32("text arena", u32::MAX as usize + 1),
            Err(StoreLimitExceeded {
                what: "text arena",
                value: u32::MAX as usize + 1
            })
        );
        // With the arena limit lowered, a push that would cross it is refused before any
        // text is written, and the builder's arena is unchanged.
        let mut b = LexiconStoreBuilder::with_capacity(2);
        b.push(
            "ab",
            "ab",
            "ab",
            "noun",
            1,
            "seed",
            [],
            [],
            FrequencyMetadata::default(),
        )
        .unwrap();
        TEST_TEXT_ARENA_LIMIT.with(|c| c.set(3));
        let err = b
            .push(
                "cde",
                "cde",
                "cde",
                "noun",
                1,
                "seed",
                [],
                [],
                FrequencyMetadata::default(),
            )
            .unwrap_err();
        TEST_TEXT_ARENA_LIMIT.with(|c| c.set(usize::MAX));
        assert_eq!(err.what, "text arena");
        assert_eq!(err.value, 5);
        assert!(err
            .to_string()
            .contains("exceeds the compact store's u32 representation"));
        assert_eq!(b.store.text, "ab");
        assert_eq!(b.store.word.len(), 1);
    }

    /// `Engine::load_lexicon` fails atomically: when the compact store cannot represent the
    /// combined lexicon, the panic leaves the engine exactly as it was (entries, order,
    /// membership, trie, query results); nothing of the attempted load is visible.
    #[test]
    fn load_lexicon_failure_leaves_the_engine_unchanged() {
        use crate::engine::Engine;
        let mut engine = Engine::new();
        engine.load_lexicon(vec![
            entry("Rojbaş", "rojbaş", "rojbaş", "seed"),
            entry("şev", "şev", "şev", "approved"),
        ]);
        let before_words: Vec<String> = engine.lexicon.words().map(str::to_string).collect();
        let snapshot = |e: &Engine| {
            (
                format!("{:?}", e.suggest("rojbas", 5)),
                format!("{:?}", e.complete("ş", 5)),
            )
        };
        let before_queries = snapshot(&engine);
        let before_attr = engine.memory_attribution();

        // Allow the existing text to be rebuilt but not the new entry's text.
        let existing_text = engine.lexicon.text.len();
        TEST_TEXT_ARENA_LIMIT.with(|c| c.set(existing_text));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            engine.load_lexicon(vec![entry("welat", "welat", "welat", "approved")]);
        }));
        TEST_TEXT_ARENA_LIMIT.with(|c| c.set(usize::MAX));
        let panic = result.expect_err("the load must fail");
        let message = panic.downcast_ref::<String>().cloned().unwrap_or_default();
        assert!(
            message.contains("compact store's u32 representation"),
            "{}",
            message
        );

        let after_words: Vec<String> = engine.lexicon.words().map(str::to_string).collect();
        assert_eq!(after_words, before_words);
        assert_eq!(engine.lexicon.len(), 2);
        assert!(engine.contains("rojbaş") && engine.contains("şev"));
        assert!(
            !engine.contains("welat"),
            "the attempted word must not be known"
        );
        assert!(engine.suggest("welat", 5).iter().all(|s| s.text != "welat"));
        assert_eq!(snapshot(&engine), before_queries);
        assert_eq!(engine.memory_attribution(), before_attr);

        // The engine is still fully usable: a representable load succeeds afterwards.
        engine.load_lexicon(vec![entry("welat", "welat", "welat", "approved")]);
        assert_eq!(engine.lexicon.len(), 3);
        assert!(engine.contains("welat"));
    }

    #[test]
    fn two_successive_load_lexicon_calls_preserve_order_and_fields() {
        use crate::engine::Engine;
        let first = vec![
            entry("Rojbaş", "rojbaş", "rojbaş", "seed"),
            entry("şev", "şev", "şev", "approved"),
        ];
        let second = vec![
            entry("bijî", "bijî", "bijîn", "seed"),
            LexiconEntry {
                regions: vec![],
                sources: vec!["kuwiki-batch-001".to_string()],
                part_of_speech: "verb".to_string(),
                frequency: 99,
                ..entry("Welat", "welat", "welat", "approved")
            },
        ];
        let mut engine = Engine::new();
        engine.load_lexicon(first.clone());
        engine.load_lexicon(second.clone());
        let expected: Vec<LexiconEntry> = first.into_iter().chain(second).collect();
        assert_eq!(engine.lexicon.len(), expected.len());
        for (i, e) in expected.iter().enumerate() {
            let back = engine.lexicon.entry(i);
            assert_eq!(back.word, e.word);
            assert_eq!(back.normalized, e.normalized);
            assert_eq!(back.lemma, e.lemma);
            assert_eq!(back.part_of_speech, e.part_of_speech);
            assert_eq!(back.frequency, e.frequency);
            assert_eq!(back.regions, e.regions);
            assert_eq!(back.status, e.status);
            assert_eq!(back.sources, e.sources);
            assert_eq!(back.frequency_metadata, e.frequency_metadata);
        }
        for e in &expected {
            assert!(engine.contains(&e.normalized), "{}", e.normalized);
            // The top suggestion is this entry (display casing follows the query's case rule).
            let top = engine.suggest(&e.normalized, 1);
            assert_eq!(crate::normalization::normalize(&top[0].text), e.normalized);
        }
        assert_eq!(engine.lexicon.position_normalized("welat"), Some(3));
    }
}
