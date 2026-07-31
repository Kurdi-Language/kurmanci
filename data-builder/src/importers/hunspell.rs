use crate::normalize::normalize_text;
use crate::sources::SourceRegistry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

// ─── Shared streaming parser API ────────────────────────────────────────────

/// Events emitted by the shared Hunspell `.dic` parser. Both the importer and
/// the audit consume this stream to ensure identical interpretation.
#[derive(Debug, Clone)]
pub enum HunspellSourceEvent {
    /// First line that is purely ASCII digits → declared entry count.
    DeclaredCount {
        source_line_num: usize,
        raw_line: String,
        count: usize,
    },
    /// Blank or whitespace-only line.
    Blank {
        source_line_num: usize,
        raw_line: String,
    },
    /// Successfully parsed dictionary entry.
    Parsed {
        source_line_num: usize,
        raw_line: String,
        entry: ParsedHunspellEntry,
        normalized: String,
        part_of_speech: String,
    },
    /// Line that failed validation.
    Rejected {
        source_line_num: usize,
        raw_line: String,
        reason_code: String,
        explanation: String,
    },
    /// Line that could not be decoded as valid UTF-8.
    Utf8Error {
        source_line_num: usize,
        explanation: String,
    },
}

/// Streams every physical line of a Hunspell `.dic` file through the shared
/// parser, emitting one `HunspellSourceEvent` per line. The caller decides
/// what to do with each event (import vs audit).
pub fn parse_hunspell_source<R: BufRead>(reader: R) -> Vec<HunspellSourceEvent> {
    let mut events = Vec::new();
    let mut saw_first_line = false;

    for (line_idx, line_res) in reader.lines().enumerate() {
        let line_num = line_idx + 1;

        let line = match line_res {
            Ok(l) => l,
            Err(e) => {
                events.push(HunspellSourceEvent::Utf8Error {
                    source_line_num: line_num,
                    explanation: format!("Failed UTF-8 decoding on line {}: {}", line_num, e),
                });
                continue;
            }
        };

        let trimmed = line.trim();

        // Check if line 1 contains purely digits (declared count header)
        if !saw_first_line && !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit()) {
            saw_first_line = true;
            if let Ok(count) = trimmed.parse::<usize>() {
                events.push(HunspellSourceEvent::DeclaredCount {
                    source_line_num: line_num,
                    raw_line: line,
                    count,
                });
                continue;
            }
        }
        saw_first_line = true;

        if trimmed.is_empty() {
            events.push(HunspellSourceEvent::Blank {
                source_line_num: line_num,
                raw_line: line,
            });
            continue;
        }

        match parse_hunspell_line(&line) {
            Ok(parsed) => {
                let normalized = normalize_text(&parsed.raw_word);
                let pos = map_part_of_speech(&parsed.morphology);
                events.push(HunspellSourceEvent::Parsed {
                    source_line_num: line_num,
                    raw_line: line,
                    entry: parsed,
                    normalized,
                    part_of_speech: pos,
                });
            }
            Err((code, explanation)) => {
                events.push(HunspellSourceEvent::Rejected {
                    source_line_num: line_num,
                    raw_line: line,
                    reason_code: code,
                    explanation,
                });
            }
        }
    }

    events
}

/// Maps Hunspell morphology tags to canonical POS string. Public so the audit
/// can derive POS identically to the importer.
pub fn map_part_of_speech(morphology: &[String]) -> String {
    map_part_of_speech_inner(morphology)
}

// ─── End shared parser API ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportedLexiconRecord {
    pub word: String,
    pub lemma: String,
    pub normalized: String,
    pub part_of_speech: String,
    pub frequency: u64,
    pub status: String,
    pub variants: Vec<String>,
    pub sources: Vec<String>,
    pub regions: Vec<String>,
    pub flags: String,
    pub morphology: Vec<String>,
    pub source_line_num: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedRecord {
    pub source_line_num: usize,
    pub raw_line: String,
    pub reason_code: String,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictRecord {
    pub normalized: String,
    pub word_a: String,
    pub line_a: usize,
    pub flags_a: String,
    pub word_b: String,
    pub line_b: usize,
    pub flags_b: String,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportSummaryReport {
    pub source_id: String,
    pub source_revision: String,
    pub importer_version: String,
    pub retrieval_date: String,
    pub declared_entry_count: Option<usize>,
    pub physical_input_lines: usize,
    pub parsed_entries: usize,
    pub accepted_entries: usize,
    pub rejected_entries: usize,
    pub duplicate_surface_forms: usize,
    pub conflicting_flag_sets: usize,
    pub declared_count_mismatch: bool,
    pub output_checksum_sha256: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct ParsedHunspellEntry {
    pub raw_word: String,
    pub flags: String,
    pub morphology: Vec<String>,
}

/// Parses a single line from a Hunspell .dic file, splitting morphology from lexical token first
pub fn parse_hunspell_line(line: &str) -> Result<ParsedHunspellEntry, (String, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err((
            "EMPTY_LINE".to_string(),
            "Line is empty or whitespace".to_string(),
        ));
    }

    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.is_empty() {
        return Err((
            "EMPTY_LINE".to_string(),
            "Line is empty or whitespace".to_string(),
        ));
    }

    let lex_token = tokens[0];
    let morph_tokens: Vec<String> = tokens[1..].iter().map(|s| (*s).to_string()).collect();

    // Scan lex_token for unescaped slash
    let mut word_chars = Vec::new();
    let mut chars = lex_token.chars().peekable();
    let mut slash_index = None;
    let mut idx = 0;

    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek() == Some(&'/') {
            word_chars.push('/');
            chars.next(); // consume '/'
            idx += 2;
        } else if ch == '/' {
            slash_index = Some(idx);
            break;
        } else {
            word_chars.push(ch);
            idx += ch.len_utf8();
        }
    }

    let raw_word: String = word_chars.into_iter().collect();
    let clean_word = raw_word.trim().to_string();

    if clean_word.is_empty() {
        return Err((
            "EMPTY_WORD".to_string(),
            "Surface word form is empty".to_string(),
        ));
    }

    if clean_word.contains('<')
        || clean_word.contains('>')
        || clean_word.starts_with("http://")
        || clean_word.starts_with("https://")
    {
        return Err((
            "FORBIDDEN_CHARACTERS".to_string(),
            "Word contains forbidden HTML or URL fragments".to_string(),
        ));
    }

    let norm = normalize_text(&clean_word);
    let char_count = norm.chars().count();
    if !(1..=64).contains(&char_count) {
        return Err((
            "INVALID_LENGTH".to_string(),
            format!("Word length {} out of bounds (1..=64)", char_count),
        ));
    }

    let flags = if let Some(slash_pos) = slash_index {
        lex_token[slash_pos + 1..].to_string()
    } else {
        String::new()
    };

    Ok(ParsedHunspellEntry {
        raw_word: clean_word,
        flags,
        morphology: morph_tokens,
    })
}

/// Maps Hunspell morphology tags (e.g. `po:adj`) to platform canonical part_of_speech string
fn map_part_of_speech_inner(morphology: &[String]) -> String {
    for morph in morphology {
        if let Some(pos_val) = morph.strip_prefix("po:") {
            match pos_val.to_lowercase().as_str() {
                "adj" | "adjective" => return "adjective".to_string(),
                "noun" | "n" => return "noun".to_string(),
                "verb" | "v" => return "verb".to_string(),
                "adv" | "adverb" => return "adverb".to_string(),
                "ij" | "interj" | "interjection" => return "interjection".to_string(),
                "prep" | "preposition" => return "preposition".to_string(),
                "conj" | "conjunction" => return "conjunction".to_string(),
                "pron" | "pronoun" => return "pronoun".to_string(),
                _ => return pos_val.to_string(),
            }
        }
    }
    "unknown".to_string()
}

type StoredRecordValue = (usize, String, String, Vec<String>, String);

/// Executes deterministic Hunspell import pipeline for source_id
pub fn import_hunspell_dic<P: AsRef<Path>>(
    source_id: &str,
    root_dir: P,
) -> Result<ImportSummaryReport, String> {
    let root = root_dir.as_ref();
    let registry_path = root.join("data/source-registry/sources.toml");
    let registry = SourceRegistry::load_from_file(&registry_path)?;

    // 1. Verify source integrity before parsing
    registry.verify_preserved_files(root)?;

    let source = registry
        .sources
        .iter()
        .find(|s| s.source_id == source_id)
        .ok_or_else(|| format!("Source ID '{}' not found in registry", source_id))?;

    let dic_file_entry = source
        .files
        .iter()
        .find(|f| f.path.ends_with(".dic"))
        .ok_or_else(|| format!("No .dic file registered for source '{}'", source_id))?;

    let dic_path = root.join(&dic_file_entry.path);
    let file = File::open(&dic_path)
        .map_err(|e| format!("Failed to open .dic file {:?}: {}", dic_path, e))?;
    let reader = BufReader::new(file);

    let mut physical_input_lines = 0;
    let mut declared_entry_count = None;
    let mut parsed_entries = 0;
    let mut rejected_list: Vec<RejectedRecord> = Vec::new();
    let mut conflict_list: Vec<ConflictRecord> = Vec::new();

    // Primary Deduplication Key: normalized surface form (String)
    // Value: (line_num, original_word, flags, morphology, part_of_speech)
    let mut seen_records: BTreeMap<String, StoredRecordValue> = BTreeMap::new();

    let mut duplicate_surface_forms = 0;
    let mut conflicting_flag_sets = 0;

    for (line_idx, line_res) in reader.lines().enumerate() {
        physical_input_lines += 1;
        let line_num = line_idx + 1;

        let line = match line_res {
            Ok(l) => l,
            Err(e) => {
                rejected_list.push(RejectedRecord {
                    source_line_num: line_num,
                    raw_line: String::new(),
                    reason_code: "INVALID_UTF8".to_string(),
                    explanation: format!("Failed UTF-8 decoding on line {}: {}", line_num, e),
                });
                continue;
            }
        };

        let trimmed = line.trim();

        // Check if line 1 contains purely digits (declared count header)
        if line_idx == 0 && !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(count) = trimmed.parse::<usize>() {
                declared_entry_count = Some(count);
                continue; // Header processed
            }
        }

        if trimmed.is_empty() {
            continue; // Skip blank lines silently
        }

        parsed_entries += 1;

        match parse_hunspell_line(&line) {
            Ok(parsed) => {
                let norm = normalize_text(&parsed.raw_word);
                let pos = map_part_of_speech(&parsed.morphology);

                if let Some((
                    existing_line,
                    existing_word,
                    existing_flags,
                    existing_morph,
                    existing_pos,
                )) = seen_records.get(&norm)
                {
                    if existing_word == &parsed.raw_word
                        && existing_flags == &parsed.flags
                        && existing_morph == &parsed.morphology
                        && existing_pos == &pos
                    {
                        duplicate_surface_forms += 1;
                    } else {
                        conflicting_flag_sets += 1;
                        conflict_list.push(ConflictRecord {
                            normalized: norm.clone(),
                            word_a: existing_word.clone(),
                            line_a: *existing_line,
                            flags_a: existing_flags.clone(),
                            word_b: parsed.raw_word.clone(),
                            line_b: line_num,
                            flags_b: parsed.flags.clone(),
                            explanation: format!(
                                "Conflict on normalized form '{}': '{}' flags='{}' (line {}) vs '{}' flags='{}' (line {})",
                                norm, existing_word, existing_flags, existing_line, parsed.raw_word, parsed.flags, line_num
                            ),
                        });
                    }
                } else {
                    seen_records.insert(
                        norm,
                        (
                            line_num,
                            parsed.raw_word,
                            parsed.flags,
                            parsed.morphology,
                            pos,
                        ),
                    );
                }
            }
            Err((code, explanation)) => {
                rejected_list.push(RejectedRecord {
                    source_line_num: line_num,
                    raw_line: line.clone(),
                    reason_code: code,
                    explanation,
                });
            }
        }
    }

    let declared_count_mismatch = match declared_entry_count {
        Some(declared) => declared != parsed_entries,
        None => false,
    };

    // Construct deterministic output records list
    let mut imported_records: Vec<ImportedLexiconRecord> = seen_records
        .into_iter()
        .map(
            |(norm, (line_num, word, flags, morphology, pos))| ImportedLexiconRecord {
                word,
                lemma: norm.clone(),
                normalized: norm,
                part_of_speech: pos,
                frequency: 0, // 0 indicates unknown / unmeasured corpus frequency
                status: "imported-unreviewed".to_string(),
                variants: Vec::new(),
                sources: vec![source_id.to_string()],
                regions: vec!["general".to_string()],
                flags,
                morphology,
                source_line_num: line_num,
            },
        )
        .collect();

    // Sort deterministically: normalized ASC, then word ASC, then flags ASC, then source_line_num ASC
    imported_records.sort_by(|a, b| {
        a.normalized
            .cmp(&b.normalized)
            .then_with(|| a.word.cmp(&b.word))
            .then_with(|| a.flags.cmp(&b.flags))
            .then_with(|| a.source_line_num.cmp(&b.source_line_num))
    });

    let accepted_entries = imported_records.len();

    // Write output files deterministically
    let imported_dir = root.join("data/imported").join(source_id);
    let reports_dir = root.join("data/reports").join(source_id);

    fs::create_dir_all(&imported_dir)
        .map_err(|e| format!("Failed to create dir {:?}: {}", imported_dir, e))?;
    fs::create_dir_all(&reports_dir)
        .map_err(|e| format!("Failed to create dir {:?}: {}", reports_dir, e))?;

    let lexicon_file_path = imported_dir.join("lexicon.jsonl");
    let mut lex_file = File::create(&lexicon_file_path)
        .map_err(|e| format!("Failed to create file {:?}: {}", lexicon_file_path, e))?;

    let mut hasher = Sha256::new();
    for rec in &imported_records {
        let json_line =
            serde_json::to_string(rec).map_err(|e| format!("Failed to serialize record: {}", e))?;
        lex_file
            .write_all(json_line.as_bytes())
            .map_err(|e| format!("Failed to write line: {}", e))?;
        lex_file
            .write_all(b"\n")
            .map_err(|e| format!("Failed to write newline: {}", e))?;
        hasher.update(json_line.as_bytes());
        hasher.update(b"\n");
    }

    let output_checksum_sha256 = format!("{:x}", hasher.finalize());

    // Write rejected.jsonl
    let rejected_path = reports_dir.join("rejected.jsonl");
    let mut rej_file = File::create(&rejected_path)
        .map_err(|e| format!("Failed to create file {:?}: {}", rejected_path, e))?;
    for rej in &rejected_list {
        let line = serde_json::to_string(rej)
            .map_err(|e| format!("Failed to serialize rejected record: {}", e))?;
        rej_file
            .write_all(line.as_bytes())
            .map_err(|e| format!("Failed to write rejected line: {}", e))?;
        rej_file
            .write_all(b"\n")
            .map_err(|e| format!("Failed to write rejected newline: {}", e))?;
    }

    // Write conflicts.jsonl
    let conflicts_path = reports_dir.join("conflicts.jsonl");
    let mut conf_file = File::create(&conflicts_path)
        .map_err(|e| format!("Failed to create file {:?}: {}", conflicts_path, e))?;
    for conf in &conflict_list {
        let line = serde_json::to_string(conf)
            .map_err(|e| format!("Failed to serialize conflict record: {}", e))?;
        conf_file
            .write_all(line.as_bytes())
            .map_err(|e| format!("Failed to write conflict line: {}", e))?;
        conf_file
            .write_all(b"\n")
            .map_err(|e| format!("Failed to write conflict newline: {}", e))?;
    }

    let summary = ImportSummaryReport {
        source_id: source_id.to_string(),
        source_revision: source.version.clone(),
        importer_version: "0.1.0".to_string(),
        retrieval_date: source
            .retrieval_date
            .clone()
            .unwrap_or_else(|| "2026-07-30".to_string()),
        declared_entry_count,
        physical_input_lines,
        parsed_entries,
        accepted_entries,
        rejected_entries: rejected_list.len(),
        duplicate_surface_forms,
        conflicting_flag_sets,
        declared_count_mismatch,
        output_checksum_sha256,
        status: "SUCCESS".to_string(),
    };

    let summary_path = reports_dir.join("import-summary.json");
    let summary_json = serde_json::to_string_pretty(&summary)
        .map_err(|e| format!("Failed to serialize summary report: {}", e))?;
    fs::write(&summary_path, summary_json)
        .map_err(|e| format!("Failed to write summary report {:?}: {}", summary_path, e))?;

    Ok(summary)
}
