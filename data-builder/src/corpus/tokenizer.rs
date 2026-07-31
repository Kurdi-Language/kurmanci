//! Deterministic Tokenizer for Kurmancî Text Corpora.
//!
//! ### Tokenization Specification & Rules
//!
//! 1. **Unicode Normalization**: Every input text line/document is normalized to Canonical Composition (NFC).
//! 2. **Case Normalization**: The normalized text is converted to lowercase. Full Kurmancî diacritics
//!    (`ç`, `ê`, `î`, `ş`, `û`) and standard Latin letters are preserved identically.
//! 3. **Word Token Boundaries**:
//!    - Splitting occurs on whitespace (`char::is_whitespace`) and Unicode General Categories `P*` (Punctuation)
//!      and `S*` (Symbol).
//!    - Apostrophes (`'`, `’`, `ʻ`) and hyphens (`-`, `–`, `—`) are treated as word boundaries, cleanly separating
//!      clitics and compound forms (e.g., `"l'amour"` -> `["l", "amour"]`, `"xwe-xwe"` -> `["xwe", "xwe"]`).
//! 4. **Number, Email, URL & Noise Filtering**:
//!    - Pure numeric tokens (e.g., `"123"`) consist only of digits and are discarded.
//!    - URLs and email addresses split at boundary characters into constituent alphanumeric tokens, with pure numbers discarded.
//!    - Tokens containing zero letter characters (`Unicode L*`) or empty strings are filtered out.

use unicode_general_category::{get_general_category, GeneralCategory};
use unicode_normalization::UnicodeNormalization;

/// Checks if a character belongs to Unicode General Category P* (Punctuation).
pub fn is_punctuation(c: char) -> bool {
    matches!(
        get_general_category(c),
        GeneralCategory::ConnectorPunctuation
            | GeneralCategory::DashPunctuation
            | GeneralCategory::OpenPunctuation
            | GeneralCategory::ClosePunctuation
            | GeneralCategory::InitialPunctuation
            | GeneralCategory::FinalPunctuation
            | GeneralCategory::OtherPunctuation
    )
}

/// Checks if a character belongs to Unicode General Category S* (Symbol).
pub fn is_symbol(c: char) -> bool {
    matches!(
        get_general_category(c),
        GeneralCategory::MathSymbol
            | GeneralCategory::CurrencySymbol
            | GeneralCategory::ModifierSymbol
            | GeneralCategory::OtherSymbol
    )
}

/// Checks if a character belongs to Unicode General Category L* (Letter).
pub fn is_letter(c: char) -> bool {
    matches!(
        get_general_category(c),
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
    )
}

/// Tokenizes a raw string input into a vector of clean, normalized Kurmancî word tokens.
pub fn tokenize_text(input: &str) -> Vec<String> {
    // 1. NFC Normalization & Lowercasing
    let nfc_lowercase: String = input.nfc().collect::<String>().to_lowercase();

    // 2. Character-by-character boundary splitting
    let mut tokens = Vec::new();
    let mut current_token = String::new();

    for c in nfc_lowercase.chars() {
        if c.is_whitespace() || is_punctuation(c) || is_symbol(c) {
            if !current_token.is_empty() {
                process_and_push_token(&current_token, &mut tokens);
                current_token.clear();
            }
        } else {
            current_token.push(c);
        }
    }

    if !current_token.is_empty() {
        process_and_push_token(&current_token, &mut tokens);
    }

    tokens
}

/// Filters and pushes valid tokens: discards pure numbers and letterless strings.
fn process_and_push_token(token: &str, out: &mut Vec<String>) {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return;
    }

    // Must contain at least one letter (Unicode L*)
    if !trimmed.chars().any(is_letter) {
        return;
    }

    // Must not be a pure number
    if trimmed.chars().all(|c| c.is_numeric()) {
        return;
    }

    out.push(trimmed.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_casing_and_punctuation() {
        assert_eq!(tokenize_text("Spas"), vec!["spas"]);
        assert_eq!(tokenize_text("SPAS"), vec!["spas"]);
        assert_eq!(tokenize_text("spas!"), vec!["spas"]);
        assert_eq!(tokenize_text("rojbaş!"), vec!["rojbaş"]);
    }

    #[test]
    fn test_kurdish_letters_preserved() {
        assert_eq!(tokenize_text("Çav"), vec!["çav"]);
        assert_eq!(tokenize_text("Şev"), vec!["şev"]);
        assert_eq!(tokenize_text("Dîwar"), vec!["dîwar"]);
        assert_eq!(tokenize_text("Êvar"), vec!["êvar"]);
        assert_eq!(tokenize_text("Hûn"), vec!["hûn"]);
    }

    #[test]
    fn test_numbers_filtered() {
        assert_eq!(tokenize_text("123"), Vec::<String>::new());
        assert_eq!(tokenize_text("sal 2026"), vec!["sal"]);
    }

    #[test]
    fn test_mixed_punctuation_and_apostrophes() {
        assert_eq!(tokenize_text("l'amour"), vec!["l", "amour"]);
        assert_eq!(tokenize_text("xwe-xwe"), vec!["xwe", "xwe"]);
        assert_eq!(tokenize_text("rojbaş, heval!"), vec!["rojbaş", "heval"]);
    }

    #[test]
    fn test_unicode_nfc_normalization() {
        // e + combining acute (e\u{0301}) -> é (\u{00E9})
        let input = "e\u{0301}var";
        let tokens = tokenize_text(input);
        assert_eq!(tokens, vec!["évar"]);
    }

    #[test]
    fn test_urls_and_emails() {
        let tokens = tokenize_text("Contact user@example.com at https://kurdi.org/docs");
        assert_eq!(
            tokens,
            vec!["contact", "user", "example", "com", "at", "https", "kurdi", "org", "docs"]
        );
    }
}
