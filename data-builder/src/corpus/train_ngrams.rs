//! In-memory bigram and trigram statistics over tokenised sentences.
//!
//! Used by `build-language-model` on the TRAIN-partition canonical representatives of one
//! corpus. Counting, probability, pruning and ordering rules are identical to
//! `build_corpus_bigrams` / `build_corpus_trigrams`.

use std::collections::BTreeMap;

use super::ngrams::{BigramRecord, TrigramRecord};
use kurmanci_engine::format::PROBABILITY_SCALE;

/// Conditional probability of `count` out of `context_count` in millionths, rounded half up
/// (the same rule as the corpus n-gram builders). Fails on impossible counts.
pub fn probability_millionths(count: u64, context_count: u64, what: &str) -> Result<u32, String> {
    if context_count == 0 || count == 0 || count > context_count {
        return Err(format!(
            "Invalid counts for {} (count {}, context {})",
            what, count, context_count
        ));
    }
    let numerator = u128::from(count)
        .checked_mul(u128::from(PROBABILITY_SCALE))
        .and_then(|v| v.checked_add(u128::from(context_count / 2)))
        .ok_or_else(|| format!("Probability overflow for {}", what))?;
    let prob = u32::try_from(numerator / u128::from(context_count))
        .map_err(|_| format!("Probability conversion overflow for {}", what))?;
    if prob > PROBABILITY_SCALE {
        return Err(format!("Probability {} exceeds scale for {}", prob, what));
    }
    Ok(prob)
}

/// Computes pruned bigram records from sentence token lists (same rules as `build_corpus_bigrams`).
pub fn compute_bigram_records(
    sentences: &[Vec<String>],
    min_count: u64,
    max_per_ctx: usize,
) -> Result<Vec<BigramRecord>, String> {
    let mut raw: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut ctx: BTreeMap<String, u64> = BTreeMap::new();
    for tokens in sentences {
        if tokens.len() < 2 {
            continue;
        }
        for w in tokens.windows(2) {
            *raw.entry((w[0].clone(), w[1].clone())).or_insert(0) += 1;
            *ctx.entry(w[0].clone()).or_insert(0) += 1;
        }
    }
    let mut grouped: BTreeMap<String, Vec<BigramRecord>> = BTreeMap::new();
    for ((prev, next), count) in raw {
        if count < min_count {
            continue;
        }
        let context_count = *ctx.get(&prev).unwrap_or(&0);
        let p = probability_millionths(
            count,
            context_count,
            &format!("bigram ({}, {})", prev, next),
        )?;
        grouped.entry(prev.clone()).or_default().push(BigramRecord {
            previous: prev,
            next,
            count,
            context_count,
            probability_millionths: p,
        });
    }
    let mut out = Vec::new();
    for (_ctx, mut recs) in grouped {
        recs.sort_by(|a, b| {
            b.probability_millionths
                .cmp(&a.probability_millionths)
                .then_with(|| b.count.cmp(&a.count))
                .then_with(|| a.next.cmp(&b.next))
        });
        recs.truncate(max_per_ctx);
        out.extend(recs);
    }
    out.sort_by(|a, b| {
        a.previous
            .cmp(&b.previous)
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.next.cmp(&b.next))
    });
    Ok(out)
}

/// Computes pruned trigram records from sentence token lists (same rules as `build_corpus_trigrams`).
pub fn compute_trigram_records(
    sentences: &[Vec<String>],
    min_count: u64,
    max_per_ctx: usize,
) -> Result<Vec<TrigramRecord>, String> {
    let mut raw: BTreeMap<(String, String, String), u64> = BTreeMap::new();
    let mut ctx: BTreeMap<(String, String), u64> = BTreeMap::new();
    for tokens in sentences {
        if tokens.len() < 3 {
            continue;
        }
        for w in tokens.windows(3) {
            *raw.entry((w[0].clone(), w[1].clone(), w[2].clone()))
                .or_insert(0) += 1;
            *ctx.entry((w[0].clone(), w[1].clone())).or_insert(0) += 1;
        }
    }
    let mut grouped: BTreeMap<(String, String), Vec<TrigramRecord>> = BTreeMap::new();
    for ((p2, p1, next), count) in raw {
        if count < min_count {
            continue;
        }
        let context_count = *ctx.get(&(p2.clone(), p1.clone())).unwrap_or(&0);
        let p = probability_millionths(
            count,
            context_count,
            &format!("trigram ({}, {}, {})", p2, p1, next),
        )?;
        grouped
            .entry((p2.clone(), p1.clone()))
            .or_default()
            .push(TrigramRecord {
                previous_2: p2,
                previous_1: p1,
                next,
                count,
                context_count,
                probability_millionths: p,
            });
    }
    let mut out = Vec::new();
    for (_ctx, mut recs) in grouped {
        recs.sort_by(|a, b| {
            b.probability_millionths
                .cmp(&a.probability_millionths)
                .then_with(|| b.count.cmp(&a.count))
                .then_with(|| a.next.cmp(&b.next))
        });
        recs.truncate(max_per_ctx);
        out.extend(recs);
    }
    out.sort_by(|a, b| {
        a.previous_2
            .cmp(&b.previous_2)
            .then_with(|| a.previous_1.cmp(&b.previous_1))
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.next.cmp(&b.next))
    });
    Ok(out)
}
