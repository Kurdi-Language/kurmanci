use crate::validate::SourceLexiconEntry;
use std::collections::BTreeMap;

pub fn merge_and_deduplicate(entries: Vec<SourceLexiconEntry>) -> Vec<SourceLexiconEntry> {
    let mut map: BTreeMap<String, SourceLexiconEntry> = BTreeMap::new();

    for entry in entries {
        let key = entry.normalized.clone();

        if let Some(existing) = map.get_mut(&key) {
            // Prefer lowercase display form for common words over capitalized forms
            if existing
                .word
                .chars()
                .next()
                .is_some_and(|c| c.is_uppercase())
                && entry.word.chars().next().is_some_and(|c| c.is_lowercase())
                && entry.part_of_speech != "proper_noun"
            {
                existing.word = entry.word.clone();
            }

            // Aggregate frequency
            if entry.frequency > existing.frequency {
                existing.frequency = entry.frequency;
            }

            // Merge sources
            for src in entry.sources {
                if !existing.sources.contains(&src) {
                    existing.sources.push(src);
                }
            }

            // Merge variants
            for v in entry.variants {
                if !existing.variants.contains(&v) {
                    existing.variants.push(v);
                }
            }
        } else {
            map.insert(key, entry);
        }
    }

    let mut result: Vec<SourceLexiconEntry> = map
        .into_values()
        .map(|mut entry| {
            // Sort source IDs deterministically
            entry.sources.sort();
            entry.sources.dedup();
            entry.variants.sort();
            entry.variants.dedup();
            entry
        })
        .collect();

    // Deterministic sorting: Frequency descending, then Normalized word ascending
    result.sort_by(|a, b| {
        b.frequency
            .cmp(&a.frequency)
            .then_with(|| a.normalized.cmp(&b.normalized))
    });

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_duplicates() {
        let e1 = SourceLexiconEntry {
            word: "rojbaş".to_string(),
            lemma: "rojbaş".to_string(),
            normalized: "rojbaş".to_string(),
            part_of_speech: "interjection".to_string(),
            frequency: 50000,
            status: "verified".to_string(),
            variants: vec![],
            sources: vec!["wiktionary".to_string()],
            regions: vec!["general".to_string()],
        };

        let e2 = SourceLexiconEntry {
            word: "Rojbaş".to_string(),
            lemma: "rojbaş".to_string(),
            normalized: "rojbaş".to_string(),
            part_of_speech: "interjection".to_string(),
            frequency: 84231,
            status: "verified".to_string(),
            variants: vec![],
            sources: vec!["manual-seed".to_string()],
            regions: vec!["general".to_string()],
        };

        let merged = merge_and_deduplicate(vec![e1, e2]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].word, "rojbaş");
        assert_eq!(merged[0].frequency, 84231);
        assert_eq!(merged[0].sources, vec!["manual-seed", "wiktionary"]);
    }

    #[test]
    fn test_deterministic_source_ordering() {
        let e1 = SourceLexiconEntry {
            word: "spas".to_string(),
            lemma: "spas".to_string(),
            normalized: "spas".to_string(),
            part_of_speech: "noun".to_string(),
            frequency: 95400,
            status: "verified".to_string(),
            variants: vec![],
            sources: vec![
                "z_source".to_string(),
                "a_source".to_string(),
                "m_source".to_string(),
            ],
            regions: vec!["general".to_string()],
        };

        let merged = merge_and_deduplicate(vec![e1]);
        assert_eq!(merged[0].sources, vec!["a_source", "m_source", "z_source"]);
    }
}
