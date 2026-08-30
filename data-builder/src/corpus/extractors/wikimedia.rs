//! Deterministic MediaWiki XML Extractor for Kurmancî Wikipedia (`kuwiki`).
//!
//! Streams decompressed bzip2 XML, extracts main-namespace article prose,
//! conservatively strips MediaWiki markup, and writes deterministic JSONL documents.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::Path;

/// Extracted source document record emitted to JSONL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtractedWikiDocument {
    pub page_id: String,
    pub title: String,
    pub text: String,
}

/// Detailed extraction summary report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikimediaExtractionReport {
    pub source_artifact: String,
    pub total_pages_seen: usize,
    pub main_namespace_pages: usize,
    pub redirect_pages_excluded: usize,
    pub non_main_pages_excluded: usize,
    pub extracted_documents: usize,
    pub total_extracted_tokens: usize,
    pub output_checksum_sha256: String,
}

/// Conservative MediaWiki wikitext markup stripper state machine.
pub fn strip_wikitext(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let len = chars.len();
    let mut i = 0;

    let mut template_depth = 0usize;
    let mut table_depth = 0usize;
    let mut in_ref = false;
    let mut in_html_tag = false;

    while i < len {
        let c = chars[i];

        // 1. Ref / References tag opening <ref ...> or <references ...>
        if !in_ref && c == '<' {
            let slice_len = len - i;
            let is_ref_prefix = slice_len >= 4
                && (chars[i + 1] == 'r' || chars[i + 1] == 'R')
                && (chars[i + 2] == 'e' || chars[i + 2] == 'E')
                && (chars[i + 3] == 'f' || chars[i + 3] == 'F');

            if is_ref_prefix {
                let is_references = slice_len >= 11
                    && (chars[i + 4] == 'e' || chars[i + 4] == 'E')
                    && (chars[i + 5] == 'r' || chars[i + 5] == 'R')
                    && (chars[i + 6] == 'e' || chars[i + 6] == 'E')
                    && (chars[i + 7] == 'n' || chars[i + 7] == 'N')
                    && (chars[i + 8] == 'c' || chars[i + 8] == 'C')
                    && (chars[i + 9] == 'e' || chars[i + 9] == 'E')
                    && (chars[i + 10] == 's' || chars[i + 10] == 'S');

                let next_char_pos = if is_references { i + 11 } else { i + 4 };
                let is_valid_tag_boundary = next_char_pos >= len || {
                    let nc = chars[next_char_pos];
                    nc == ' ' || nc == '>' || nc == '/' || nc == '\t' || nc == '\n' || nc == '\r'
                };

                if is_valid_tag_boundary {
                    // Find closing '>' of opening tag
                    let mut tag_end = i;
                    while tag_end < len && chars[tag_end] != '>' {
                        tag_end += 1;
                    }

                    let mut is_self_closing = false;
                    if tag_end < len && chars[tag_end] == '>' {
                        let mut p = tag_end;
                        while p > i
                            && (chars[p - 1] == ' '
                                || chars[p - 1] == '\t'
                                || chars[p - 1] == '\n'
                                || chars[p - 1] == '\r')
                        {
                            p -= 1;
                        }
                        if p > i && chars[p - 1] == '/' {
                            is_self_closing = true;
                        }
                    }

                    if is_self_closing || is_references {
                        i = if tag_end < len { tag_end + 1 } else { len };
                        continue;
                    } else {
                        in_ref = true;
                        i = if tag_end < len { tag_end + 1 } else { len };
                        continue;
                    }
                }
            }
        }

        // Ref tag closing </ref>
        if in_ref {
            if c == '<'
                && i + 5 < len
                && chars[i + 1] == '/'
                && (chars[i + 2] == 'r' || chars[i + 2] == 'R')
                && (chars[i + 3] == 'e' || chars[i + 3] == 'E')
                && (chars[i + 4] == 'f' || chars[i + 4] == 'F')
            {
                let mut tag_end = i + 5;
                while tag_end < len && chars[tag_end] != '>' {
                    tag_end += 1;
                }
                in_ref = false;
                i = if tag_end < len { tag_end + 1 } else { len };
            } else {
                i += 1;
            }
            continue;
        }

        // 2. Templates {{ ... }}
        if i + 1 < len && c == '{' && chars[i + 1] == '{' {
            template_depth += 1;
            i += 2;
            continue;
        }
        if template_depth > 0 && i + 1 < len && c == '}' && chars[i + 1] == '}' {
            template_depth -= 1;
            i += 2;
            continue;
        }
        if template_depth > 0 {
            i += 1;
            continue;
        }

        // 3. Tables {| ... |}
        if i + 1 < len && c == '{' && chars[i + 1] == '|' {
            table_depth += 1;
            i += 2;
            continue;
        }
        if table_depth > 0 && i + 1 < len && c == '|' && chars[i + 1] == '}' {
            table_depth -= 1;
            i += 2;
            continue;
        }
        if table_depth > 0 {
            i += 1;
            continue;
        }

        // 4. HTML tags <...>
        if c == '<' {
            in_html_tag = true;
            i += 1;
            continue;
        }
        if in_html_tag {
            if c == '>' {
                in_html_tag = false;
            }
            i += 1;
            continue;
        }

        // 5. Internal / External Links [[ ... ]]
        if i + 1 < len && c == '[' && chars[i + 1] == '[' {
            let mut j = i + 2;
            let mut link_content = String::new();
            while j + 1 < len && !(chars[j] == ']' && chars[j + 1] == ']') {
                link_content.push(chars[j]);
                j += 1;
            }

            let lower = link_content.to_lowercase();
            if lower.starts_with("kategori:")
                || lower.starts_with("category:")
                || lower.starts_with("wêne:")
                || lower.starts_with("file:")
                || lower.starts_with("image:")
                || lower.starts_with("en:")
                || lower.starts_with("tr:")
            {
                // Discard metadata link entirely
            } else {
                // Extract visible display text if pipe present [[target|display]]
                if let Some(pipe_pos) = link_content.find('|') {
                    out.push_str(&link_content[pipe_pos + 1..]);
                } else {
                    out.push_str(&link_content);
                }
            }

            i = j + 2;
            continue;
        }

        // 6. Heading markers == ... ==
        if c == '=' {
            i += 1;
            continue;
        }

        // Preserved character
        out.push(c);
        i += 1;
    }

    let mut cleaned_lines = Vec::new();
    for l in out.lines() {
        let trimmed = l.trim();
        if !trimmed.is_empty() {
            cleaned_lines.push(trimmed);
        }
    }
    cleaned_lines.join("\n")
}

/// Parses a MediaWiki XML stream and extracts main-namespace article JSONL documents.
pub fn extract_wikimedia_xml<R: Read>(
    reader: R,
    source_artifact_name: &str,
    writer: &mut impl Write,
) -> Result<WikimediaExtractionReport, String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut xml_reader = Reader::from_reader(BufReader::new(reader));
    xml_reader.trim_text(true);

    let mut buf = Vec::new();

    let mut total_pages = 0usize;
    let mut main_ns_pages = 0usize;
    let mut redirect_excluded = 0usize;
    let mut non_main_excluded = 0usize;
    let mut extracted_docs = 0usize;
    let mut total_tokens = 0usize;

    let mut hasher = Sha256::new();

    let mut in_page = false;
    let mut in_title = false;
    let mut in_ns = false;
    let mut in_id = false;
    let mut in_text = false;

    let mut current_title = String::new();
    let mut current_ns = String::new();
    let mut current_id = String::new();
    let mut current_text = String::new();
    let mut is_redirect = false;
    let mut saw_page_id = false;

    loop {
        match xml_reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.name().as_ref() {
                b"page" => {
                    in_page = true;
                    total_pages += 1;
                    current_title.clear();
                    current_ns.clear();
                    current_id.clear();
                    current_text.clear();
                    is_redirect = false;
                    saw_page_id = false;
                }
                b"title" if in_page => in_title = true,
                b"ns" if in_page => in_ns = true,
                b"id" if in_page && !saw_page_id => in_id = true,
                b"text" if in_page => in_text = true,
                b"redirect" if in_page => is_redirect = true,
                _ => {}
            },
            Ok(Event::Empty(ref e)) => {
                if in_page && e.name().as_ref() == b"redirect" {
                    is_redirect = true;
                }
            }
            Ok(Event::Text(e)) => {
                let text = e
                    .unescape()
                    .map_err(|err| format!("XML unescape error: {}", err))?;
                if in_title {
                    current_title.push_str(&text);
                } else if in_ns {
                    current_ns.push_str(&text);
                } else if in_id {
                    current_id.push_str(&text);
                } else if in_text {
                    current_text.push_str(&text);
                }
            }
            Ok(Event::End(ref e)) => match e.name().as_ref() {
                b"title" => in_title = false,
                b"ns" => in_ns = false,
                b"id" => {
                    if in_id {
                        saw_page_id = true;
                        in_id = false;
                    }
                }
                b"text" => in_text = false,
                b"page" => {
                    in_page = false;

                    if is_redirect {
                        redirect_excluded += 1;
                    } else if current_ns.trim() != "0" {
                        non_main_excluded += 1;
                    } else {
                        main_ns_pages += 1;
                        let prose = strip_wikitext(&current_text);

                        if !prose.trim().is_empty() {
                            extracted_docs += 1;
                            let doc_tokens = prose.split_whitespace().count();
                            total_tokens += doc_tokens;

                            let doc_rec = ExtractedWikiDocument {
                                page_id: current_id.trim().to_string(),
                                title: current_title.trim().to_string(),
                                text: prose,
                            };

                            let json_line = serde_json::to_string(&doc_rec)
                                .map_err(|err| format!("JSON serialization error: {}", err))?;
                            writer
                                .write_all(json_line.as_bytes())
                                .map_err(|err| format!("Write error: {}", err))?;
                            writer
                                .write_all(b"\n")
                                .map_err(|err| format!("Write error: {}", err))?;

                            hasher.update(json_line.as_bytes());
                            hasher.update(b"\n");
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error: {}", e)),
            _ => {}
        }
        buf.clear();
    }

    let output_checksum_sha256 = format!("{:x}", hasher.finalize());

    Ok(WikimediaExtractionReport {
        source_artifact: source_artifact_name.to_string(),
        total_pages_seen: total_pages,
        main_namespace_pages: main_ns_pages,
        redirect_pages_excluded: redirect_excluded,
        non_main_pages_excluded: non_main_excluded,
        extracted_documents: extracted_docs,
        total_extracted_tokens: total_tokens,
        output_checksum_sha256,
    })
}

/// Executes file-based extraction from local .xml.bz2 or .xml artifact to output JSONL path.
pub fn extract_wikimedia_file<P: AsRef<Path>>(
    input_path: P,
    output_jsonl_path: P,
    expected_sha1: Option<&str>,
) -> Result<WikimediaExtractionReport, String> {
    let in_p = input_path.as_ref();
    let out_p = output_jsonl_path.as_ref();

    if !in_p.exists() {
        return Err(format!("Input Wikimedia artifact missing at {:?}", in_p));
    }

    // Verify upstream SHA-1 if provided
    if let Some(expected_sha1_hex) = expected_sha1 {
        let mut f = File::open(in_p).map_err(|e| format!("Failed to open {:?}: {}", in_p, e))?;
        let mut sha1_hasher = sha1::Sha1::new();
        let mut buf = [0u8; 16384];
        loop {
            let n = f.read(&mut buf).map_err(|e| format!("Read error: {}", e))?;
            if n == 0 {
                break;
            }
            sha1_hasher.update(&buf[..n]);
        }
        let actual_sha1 = format!("{:x}", sha1_hasher.finalize());
        if actual_sha1 != expected_sha1_hex {
            return Err(format!(
                "Upstream SHA-1 checksum mismatch for {:?}: expected {}, found {}",
                in_p, expected_sha1_hex, actual_sha1
            ));
        }
    }

    let parent_dir = out_p.parent().unwrap_or_else(|| Path::new("."));
    if !parent_dir.exists() {
        fs::create_dir_all(parent_dir)
            .map_err(|e| format!("Failed to create parent dir {:?}: {}", parent_dir, e))?;
    }

    let file_stem = out_p.file_name().unwrap_or_default().to_string_lossy();
    let tmp_p = parent_dir.join(format!(".{}.tmp_stage_{}", file_stem, std::process::id()));

    let file = File::open(in_p).map_err(|e| format!("Failed to open {:?}: {}", in_p, e))?;
    let mut out_file = File::create(&tmp_p)
        .map_err(|e| format!("Failed to create stage file {:?}: {}", tmp_p, e))?;

    let filename = in_p
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let result = if filename.ends_with(".bz2") {
        let decompressor = bzip2::read::BzDecoder::new(file);
        extract_wikimedia_xml(decompressor, &filename, &mut out_file)
    } else {
        extract_wikimedia_xml(file, &filename, &mut out_file)
    };

    drop(out_file);

    match result {
        Ok(report) => {
            fs::rename(&tmp_p, out_p)
                .map_err(|e| format!("Failed to rename stage file to {:?}: {}", out_p, e))?;
            Ok(report)
        }
        Err(err) => {
            if tmp_p.exists() {
                let _ = fs::remove_file(&tmp_p);
            }
            Err(err)
        }
    }
}
