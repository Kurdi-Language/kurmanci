//! Spelling-side quality of a built pack on a held-out partition (`pack-quality-v1`).
//!
//! Four families of measurement, all mechanical, so comparing packs costs no human judgement:
//! how much of real held-out text the pack knows, how well it completes a word from its first
//! letters, whether it restores a word from a deterministically corrupted spelling, and whether
//! it leaves correctly spelled words alone. Nothing here ranks, decides or changes anything; the
//! report carries counts, rates and hashes.
//!
//! # Why both directions of correction
//!
//! A larger vocabulary flatters a one-directional correction measurement: it knows more words,
//! so it offers corrections for fewer inputs, and the corrections it does offer are for inputs
//! that really are misspelled. Measured that way, every pack improves by growing. So this
//! reports the opposite direction too: for held-out words that are spelled correctly, how often
//! the pack treats them as unknown, and how often it offers a correction anyway. A pack is only
//! better if it restores more corrupted words *without* flagging more correct ones.
//!
//! # Why the targets do not depend on the pack
//!
//! Completion and correction targets are drawn from the corpus, in a deterministic order, before
//! any pack is consulted. If each pack were measured on the words it happens to know, a pack that
//! knows fewer words would be measured on an easier set and the comparison would be meaningless.
//! Every pack therefore answers for exactly the same targets, and a target the pack does not know
//! counts as a miss rather than being dropped.
//!
//! # What a comparison across packs can and cannot attribute
//!
//! These four families depend on the lexicon alone, so a difference between packs is a difference
//! in vocabulary. That is not true of next-word prediction, where the pack compiler embeds only
//! the n-gram records whose words are in the lexicon, so vocabulary and table size move together;
//! that measurement lives in `eval_prediction_coverage` with the caveat recorded there.

use crate::corpus::partition::PartitionDocumentRecord;
use crate::corpus::tokenizer::tokenize_text;
use crate::eval_support::verify_pack_on_held_out_partition;
use kurmanci_engine::{CompletionOptions, CorrectionOptions, KurmanciEngine};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const PACK_QUALITY_SCHEMA_VERSION: &str = "pack-quality-v1";

/// Prefix lengths completion is measured at, in characters of the normalized form.
const PREFIX_LENGTHS: [usize; 4] = [1, 2, 3, 4];
/// A target must be at least this long to be corrupted or completed meaningfully.
const MIN_TARGET_CHARS: usize = 4;
/// Candidates requested per query. Fixed, so the top-k rates of two reports are comparable.
const RESULT_LIMIT: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackQualityReport {
    pub schema_version: String,
    pub provenance: QualityProvenance,
    pub corpus: CorpusCounts,
    pub known_words: KnownWords,
    /// One row per prefix length, in increasing order.
    pub completion: Vec<CompletionRow>,
    /// One row per corruption class, in a fixed order.
    pub correction: Vec<CorrectionRow>,
    pub clean_input: CleanInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QualityProvenance {
    pub pack_path: String,
    pub pack_sha256: String,
    pub pack_entry_count: usize,
    pub pack_manifest_sha256: String,
    pub model_id: String,
    pub model_manifest_sha256: String,
    pub corpus_id: String,
    pub partition: String,
    pub partition_manifest_sha256: String,
    pub partition_sha256: String,
    pub partition_declared_documents: usize,
    pub result_limit: usize,
    pub target_limit: usize,
    pub min_target_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CorpusCounts {
    pub documents_read: usize,
    pub documents_evaluated: usize,
    pub other_corpus_documents_skipped: usize,
    pub duplicate_documents_skipped: usize,
}

/// Coverage of held-out text, counted two ways. Token coverage is what a person typing would
/// feel, since a common word counts once per occurrence; form coverage is what a lexicon
/// comparison usually quotes. They answer different questions and both are reported.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct KnownWords {
    pub token_occurrences: usize,
    pub token_occurrences_known: usize,
    pub distinct_forms: usize,
    pub distinct_forms_known: usize,
    /// Distinct forms outside the 31-letter alphabet, which no pack may contain by policy, so
    /// they are reported rather than counted against a pack.
    pub distinct_forms_outside_alphabet: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompletionRow {
    pub prefix_chars: usize,
    pub targets: usize,
    pub top1: usize,
    pub top3: usize,
    pub top5: usize,
    pub no_candidate: usize,
    /// Summed candidate count, so a mean can be derived without storing per-target data.
    pub candidates_returned: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CorrectionRow {
    pub corruption: String,
    pub targets: usize,
    /// Corruptions that happen to be a word the pack knows. The pack cannot be asked to correct
    /// these, and counting them as failures would punish a pack for knowing more words.
    pub corruption_is_known_word: usize,
    pub measured: usize,
    pub top1: usize,
    pub top3: usize,
    pub top5: usize,
    pub no_suggestion: usize,
}

/// The other direction: what the pack does to words that are already correct.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CleanInput {
    pub targets: usize,
    /// Targets the pack knows. This is the ceiling for every correction rate: a target the pack
    /// does not contain can never be restored, so a correction comparison between packs is
    /// mostly a vocabulary comparison and should be read as one.
    pub targets_known: usize,
    pub treated_as_unknown: usize,
    /// Known input for which a correction other than the input itself is still offered first.
    pub corrected_away_from_input: usize,
}

/// A deterministic corruption of a correctly spelled word. Each is a single edit, applied at a
/// position derived from the word itself, so the same corpus always yields the same corruptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Corruption {
    DeleteOneLetter,
    DuplicateOneLetter,
    SubstituteOneLetter,
    TransposeTwoLetters,
    StripDiacritics,
}

impl Corruption {
    const ALL: [Corruption; 5] = [
        Corruption::DeleteOneLetter,
        Corruption::DuplicateOneLetter,
        Corruption::SubstituteOneLetter,
        Corruption::TransposeTwoLetters,
        Corruption::StripDiacritics,
    ];

    fn name(self) -> &'static str {
        match self {
            Corruption::DeleteOneLetter => "delete_one_letter",
            Corruption::DuplicateOneLetter => "duplicate_one_letter",
            Corruption::SubstituteOneLetter => "substitute_one_letter",
            Corruption::TransposeTwoLetters => "transpose_two_letters",
            Corruption::StripDiacritics => "strip_diacritics",
        }
    }

    /// Returns the corrupted form, or `None` when this word cannot carry this corruption (for
    /// example stripping diacritics from a word that has none, which would be no corruption at
    /// all and must not be counted as a trivially passed case).
    fn apply(self, word: &str) -> Option<String> {
        let chars: Vec<char> = word.chars().collect();
        if chars.len() < MIN_TARGET_CHARS {
            return None;
        }
        // The position is the middle of the word, so it is a function of the word alone and
        // never of iteration order or a random seed.
        let at = chars.len() / 2;
        let out: String = match self {
            Corruption::DeleteOneLetter => chars
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != at)
                .map(|(_, c)| *c)
                .collect(),
            Corruption::DuplicateOneLetter => {
                let mut v = chars.clone();
                v.insert(at, chars[at]);
                v.into_iter().collect()
            }
            Corruption::SubstituteOneLetter => {
                // Substitute with a letter of the alphabet that is not the one already there,
                // chosen by position so the result is deterministic.
                let alphabet = crate::alphabet::KURMANCI_ALPHABET;
                let replacement = alphabet
                    .iter()
                    .copied()
                    .find(|c| *c != chars[at])
                    .expect("the alphabet has more than one letter");
                let mut v = chars.clone();
                v[at] = replacement;
                v.into_iter().collect()
            }
            Corruption::TransposeTwoLetters => {
                let mut v = chars.clone();
                if at + 1 >= v.len() || v[at] == v[at + 1] {
                    return None;
                }
                v.swap(at, at + 1);
                v.into_iter().collect()
            }
            Corruption::StripDiacritics => kurmanci_engine::strip_diacritics(word),
        };
        if out == word || out.chars().count() < 2 {
            None
        } else {
            Some(out)
        }
    }
}

fn rate(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 / whole as f64) * 100.0
    }
}

/// Measures the pack at `pack_path` on `partition`. `target_limit` caps the number of distinct
/// forms used as completion and correction targets; coverage always uses the whole partition.
pub fn evaluate_pack_quality(
    root: &Path,
    pack_path: &Path,
    partition: &str,
    target_limit: usize,
) -> Result<PackQualityReport, String> {
    if target_limit == 0 {
        return Err("target_limit must be at least 1".to_string());
    }
    let verified = verify_pack_on_held_out_partition(root, pack_path, partition)?;

    let engine = KurmanciEngine::from_pack_bytes(&verified.pack_bytes)
        .map_err(|e| format!("Failed to load pack {:?}: {}", pack_path, e))?;
    let pack_entry_count = engine.len();

    // 1. Walk the held-out partition once, with the same corpus scoping and canonical
    //    representative rule every other consumer applies, and with the model builder's
    //    tokenizer.
    let mut corpus = CorpusCounts::default();
    let mut form_counts: BTreeMap<String, usize> = BTreeMap::new();
    for (idx, line) in verified
        .partition_bytes
        .split(|b| *b == b'\n')
        .enumerate()
        .filter(|(_, l)| !l.is_empty())
    {
        let text = std::str::from_utf8(line)
            .map_err(|e| format!("partition line {} is not UTF-8: {}", idx + 1, e))?;
        if text.trim().is_empty() {
            continue;
        }
        let record: PartitionDocumentRecord = serde_json::from_str(text)
            .map_err(|e| format!("JSON error in partition line {}: {}", idx + 1, e))?;
        if record.partition != partition {
            return Err(format!(
                "partition file line {} declares partition '{}', expected '{}'",
                idx + 1,
                record.partition,
                partition
            ));
        }
        corpus.documents_read += 1;
        if record.corpus_id != verified.corpus_id {
            corpus.other_corpus_documents_skipped += 1;
            continue;
        }
        if record.corpus_id != record.canonical_corpus_id
            || record.document_id != record.canonical_document_id
        {
            corpus.duplicate_documents_skipped += 1;
            continue;
        }
        corpus.documents_evaluated += 1;
        for token in tokenize_text(&record.text) {
            *form_counts.entry(token).or_insert(0) += 1;
        }
    }
    if corpus.documents_read != verified.partition_declared_documents {
        return Err(format!(
            "read {} documents from the {} partition but its manifest declares {}",
            corpus.documents_read, partition, verified.partition_declared_documents
        ));
    }

    // 2. Coverage of held-out text, by occurrence and by distinct form.
    let mut known = KnownWords::default();
    for (form, count) in &form_counts {
        known.token_occurrences += count;
        known.distinct_forms += 1;
        if crate::alphabet::default_pack_eligibility(form).is_err() {
            known.distinct_forms_outside_alphabet += 1;
        }
        if engine.is_known_word(form) {
            known.distinct_forms_known += 1;
            known.token_occurrences_known += count;
        }
    }

    // 3. Targets: alphabet-eligible forms long enough to corrupt, most frequent first and then
    //    alphabetically, so the set is a deterministic function of the partition alone and is
    //    identical for every pack measured on it.
    let mut eligible: Vec<(&String, usize)> = form_counts
        .iter()
        .filter(|(form, _)| {
            form.chars().count() >= MIN_TARGET_CHARS
                && crate::alphabet::default_pack_eligibility(form).is_ok()
        })
        .map(|(f, c)| (f, *c))
        .collect();
    eligible.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let targets: Vec<&str> = eligible
        .into_iter()
        .take(target_limit)
        .map(|(f, _)| f.as_str())
        .collect();

    // 4. Completion from the first n letters.
    let mut completion = Vec::new();
    for prefix_chars in PREFIX_LENGTHS {
        let mut row = CompletionRow {
            prefix_chars,
            targets: 0,
            top1: 0,
            top3: 0,
            top5: 0,
            no_candidate: 0,
            candidates_returned: 0,
        };
        for target in &targets {
            let prefix: String = target.chars().take(prefix_chars).collect();
            if prefix.chars().count() < prefix_chars {
                continue;
            }
            row.targets += 1;
            let results = engine.complete(
                &prefix,
                CompletionOptions {
                    limit: RESULT_LIMIT,
                },
            );
            row.candidates_returned += results.len();
            if results.is_empty() {
                row.no_candidate += 1;
            }
            match results.iter().position(|r| r.text == **target) {
                Some(0) => {
                    row.top1 += 1;
                    row.top3 += 1;
                    row.top5 += 1;
                }
                Some(i) if i < 3 => {
                    row.top3 += 1;
                    row.top5 += 1;
                }
                Some(_) => row.top5 += 1,
                None => {}
            }
        }
        completion.push(row);
    }

    // 5. Correction of a deterministically corrupted spelling.
    let mut correction = Vec::new();
    for corruption in Corruption::ALL {
        let mut row = CorrectionRow {
            corruption: corruption.name().to_string(),
            targets: 0,
            corruption_is_known_word: 0,
            measured: 0,
            top1: 0,
            top3: 0,
            top5: 0,
            no_suggestion: 0,
        };
        for target in &targets {
            let Some(corrupted) = corruption.apply(target) else {
                continue;
            };
            row.targets += 1;
            // A corruption that is itself a word cannot be asked to be corrected. Counting it
            // as a failure would mean a pack is penalised for knowing more words.
            if engine.is_known_word(&corrupted) {
                row.corruption_is_known_word += 1;
                continue;
            }
            row.measured += 1;
            let results = engine.correct(
                &corrupted,
                CorrectionOptions {
                    limit: RESULT_LIMIT,
                },
            );
            if results.is_empty() {
                row.no_suggestion += 1;
            }
            match results.iter().position(|r| r.text == **target) {
                Some(0) => {
                    row.top1 += 1;
                    row.top3 += 1;
                    row.top5 += 1;
                }
                Some(i) if i < 3 => {
                    row.top3 += 1;
                    row.top5 += 1;
                }
                Some(_) => row.top5 += 1,
                None => {}
            }
        }
        correction.push(row);
    }

    // 6. The other direction: correctly spelled held-out words must be left alone.
    let mut clean_input = CleanInput::default();
    for target in &targets {
        clean_input.targets += 1;
        if !engine.is_known_word(target) {
            clean_input.treated_as_unknown += 1;
            continue;
        }
        clean_input.targets_known += 1;
        let results = engine.correct(
            target,
            CorrectionOptions {
                limit: RESULT_LIMIT,
            },
        );
        if let Some(first) = results.first() {
            if first.text != **target {
                clean_input.corrected_away_from_input += 1;
            }
        }
    }

    Ok(PackQualityReport {
        schema_version: PACK_QUALITY_SCHEMA_VERSION.to_string(),
        provenance: QualityProvenance {
            pack_path: pack_path.display().to_string(),
            pack_sha256: verified.pack_sha256,
            pack_entry_count,
            pack_manifest_sha256: verified.pack_manifest_sha256,
            model_id: verified.model_id,
            model_manifest_sha256: verified.model_manifest_sha256,
            corpus_id: verified.corpus_id,
            partition: verified.partition,
            partition_manifest_sha256: verified.partition_manifest_sha256,
            partition_sha256: verified.partition_sha256,
            partition_declared_documents: verified.partition_declared_documents,
            result_limit: RESULT_LIMIT,
            target_limit,
            min_target_chars: MIN_TARGET_CHARS,
        },
        corpus,
        known_words: known,
        completion,
        correction,
        clean_input,
    })
}

/// Fixed-width text rendering. Counts, rates and hashes only.
pub fn format_report(r: &PackQualityReport) -> String {
    let p = &r.provenance;
    let k = &r.known_words;
    let mut out = format!(
        "Pack quality ({})\n  pack {} (sha256 {}…, {} entries), model {}\n  partition {} ({} documents declared, {} evaluated); targets {}, limit {}\n",
        r.schema_version,
        p.pack_path,
        &p.pack_sha256[..12],
        p.pack_entry_count,
        p.model_id,
        p.partition,
        p.partition_declared_documents,
        r.corpus.documents_evaluated,
        p.target_limit,
        p.result_limit
    );
    out.push_str(&format!(
        "\nknown words\n  {:<34} {:>9} / {:<9} {:>6.1}%\n  {:<34} {:>9} / {:<9} {:>6.1}%\n  {:<34} {:>9}\n",
        "token occurrences known",
        k.token_occurrences_known,
        k.token_occurrences,
        rate(k.token_occurrences_known, k.token_occurrences),
        "distinct forms known",
        k.distinct_forms_known,
        k.distinct_forms,
        rate(k.distinct_forms_known, k.distinct_forms),
        "distinct forms outside the alphabet",
        k.distinct_forms_outside_alphabet
    ));
    out.push_str("\ncompletion from the first letters\n  prefix  targets    top1    top3    top5  none  mean cands\n");
    for c in &r.completion {
        out.push_str(&format!(
            "  {:>6} {:>8} {:>6.1}% {:>6.1}% {:>6.1}% {:>5} {:>11.2}\n",
            c.prefix_chars,
            c.targets,
            rate(c.top1, c.targets),
            rate(c.top3, c.targets),
            rate(c.top5, c.targets),
            c.no_candidate,
            if c.targets == 0 {
                0.0
            } else {
                c.candidates_returned as f64 / c.targets as f64
            }
        ));
    }
    out.push_str(&format!(
        "\ncorrection of a corrupted spelling (ceiling {:.1}%: the share of targets this pack knows)\n  corruption             measured    top1    top3    top5  none  already a word\n",
        rate(r.clean_input.targets_known, r.clean_input.targets)
    ));
    for c in &r.correction {
        out.push_str(&format!(
            "  {:<22} {:>8} {:>6.1}% {:>6.1}% {:>6.1}% {:>5} {:>15}\n",
            c.corruption,
            c.measured,
            rate(c.top1, c.measured),
            rate(c.top3, c.measured),
            rate(c.top5, c.measured),
            c.no_suggestion,
            c.corruption_is_known_word
        ));
    }
    let ci = &r.clean_input;
    out.push_str(&format!(
        "\ncorrectly spelled input\n  {:<34} {:>9} {:>7.1}%\n  {:<34} {:>9} {:>7.1}%\n",
        "treated as unknown",
        ci.treated_as_unknown,
        rate(ci.treated_as_unknown, ci.targets),
        "first suggestion is another word",
        ci.corrected_away_from_input,
        rate(ci.corrected_away_from_input, ci.targets)
    ));
    out
}
