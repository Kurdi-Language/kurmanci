//! Deterministic classification functions for Unicode properties, script
//! detection, shape analysis, and structural checks.

use unicode_general_category::{get_general_category, GeneralCategory};
use unicode_script::{Script, UnicodeScript};
use unicode_segmentation::UnicodeSegmentation;

// ─── Unicode general category helpers ───────────────────────────────────────

/// Returns true if the character has Unicode General Category P* (any punctuation).
pub fn is_unicode_punctuation(ch: char) -> bool {
    matches!(
        get_general_category(ch),
        GeneralCategory::ConnectorPunctuation
            | GeneralCategory::DashPunctuation
            | GeneralCategory::ClosePunctuation
            | GeneralCategory::FinalPunctuation
            | GeneralCategory::InitialPunctuation
            | GeneralCategory::OpenPunctuation
            | GeneralCategory::OtherPunctuation
    )
}

/// Returns true if the character has Unicode General Category S* (any symbol).
pub fn is_unicode_symbol(ch: char) -> bool {
    matches!(
        get_general_category(ch),
        GeneralCategory::CurrencySymbol
            | GeneralCategory::ModifierSymbol
            | GeneralCategory::MathSymbol
            | GeneralCategory::OtherSymbol
    )
}

/// Returns true if the character has Unicode General Category N* (any number).
pub fn is_unicode_number(ch: char) -> bool {
    matches!(
        get_general_category(ch),
        GeneralCategory::DecimalNumber
            | GeneralCategory::LetterNumber
            | GeneralCategory::OtherNumber
    )
}

/// Returns true if the character has Unicode General Category L* (any letter).
pub fn is_unicode_letter(ch: char) -> bool {
    matches!(
        get_general_category(ch),
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
    )
}

/// Returns true if the character is a Unicode control character (Cc).
pub fn is_unicode_control(ch: char) -> bool {
    matches!(get_general_category(ch), GeneralCategory::Control)
}

/// Returns the Unicode general category as a stable string.
pub fn general_category_str(ch: char) -> &'static str {
    match get_general_category(ch) {
        GeneralCategory::UppercaseLetter => "Lu",
        GeneralCategory::LowercaseLetter => "Ll",
        GeneralCategory::TitlecaseLetter => "Lt",
        GeneralCategory::ModifierLetter => "Lm",
        GeneralCategory::OtherLetter => "Lo",
        GeneralCategory::NonspacingMark => "Mn",
        GeneralCategory::SpacingMark => "Mc",
        GeneralCategory::EnclosingMark => "Me",
        GeneralCategory::DecimalNumber => "Nd",
        GeneralCategory::LetterNumber => "Nl",
        GeneralCategory::OtherNumber => "No",
        GeneralCategory::ConnectorPunctuation => "Pc",
        GeneralCategory::DashPunctuation => "Pd",
        GeneralCategory::OpenPunctuation => "Ps",
        GeneralCategory::ClosePunctuation => "Pe",
        GeneralCategory::InitialPunctuation => "Pi",
        GeneralCategory::FinalPunctuation => "Pf",
        GeneralCategory::OtherPunctuation => "Po",
        GeneralCategory::MathSymbol => "Sm",
        GeneralCategory::CurrencySymbol => "Sc",
        GeneralCategory::ModifierSymbol => "Sk",
        GeneralCategory::OtherSymbol => "So",
        GeneralCategory::SpaceSeparator => "Zs",
        GeneralCategory::LineSeparator => "Zl",
        GeneralCategory::ParagraphSeparator => "Zp",
        GeneralCategory::Control => "Cc",
        GeneralCategory::Format => "Cf",
        GeneralCategory::Surrogate => "Cs",
        GeneralCategory::PrivateUse => "Co",
        GeneralCategory::Unassigned => "Cn",
        _ => "Xx",
    }
}

// ─── Script classification ──────────────────────────────────────────────────

/// Classifies an entry by script composition. Ignores Common and Inherited
/// scripts (digits, punctuation, combining marks).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScriptClass {
    LatinOnly,
    ArabicOnly,
    CyrillicOnly,
    OtherSingleScript(String),
    LatinArabicMixed,
    LatinCyrillicMixed,
    OtherMixedScript,
    NoScriptedLetters,
}

impl ScriptClass {
    pub fn as_str(&self) -> String {
        match self {
            Self::LatinOnly => "LATIN_ONLY".to_string(),
            Self::ArabicOnly => "ARABIC_ONLY".to_string(),
            Self::CyrillicOnly => "CYRILLIC_ONLY".to_string(),
            Self::OtherSingleScript(s) => format!("OTHER_SINGLE_SCRIPT:{}", s),
            Self::LatinArabicMixed => "LATIN_ARABIC_MIXED".to_string(),
            Self::LatinCyrillicMixed => "LATIN_CYRILLIC_MIXED".to_string(),
            Self::OtherMixedScript => "OTHER_MIXED_SCRIPT".to_string(),
            Self::NoScriptedLetters => "NO_SCRIPTED_LETTERS".to_string(),
        }
    }
}

/// Classifies a word by the scripts of its alphabetic characters.
pub fn classify_script(word: &str) -> ScriptClass {
    let mut has_latin = false;
    let mut has_arabic = false;
    let mut has_cyrillic = false;
    let mut has_other_script = false;
    let mut other_script_name: Option<String> = None;
    let mut has_any_scripted = false;

    for ch in word.chars() {
        if !is_unicode_letter(ch) {
            continue;
        }
        let script = ch.script();
        if script == Script::Common || script == Script::Inherited {
            continue;
        }
        has_any_scripted = true;
        match script {
            Script::Latin => has_latin = true,
            Script::Arabic => has_arabic = true,
            Script::Cyrillic => has_cyrillic = true,
            other => {
                has_other_script = true;
                if other_script_name.is_none() {
                    other_script_name = Some(format!("{:?}", other));
                }
            }
        }
    }

    if !has_any_scripted {
        return ScriptClass::NoScriptedLetters;
    }

    let script_count = [has_latin, has_arabic, has_cyrillic, has_other_script]
        .iter()
        .filter(|&&v| v)
        .count();

    if script_count == 1 {
        if has_latin {
            return ScriptClass::LatinOnly;
        }
        if has_arabic {
            return ScriptClass::ArabicOnly;
        }
        if has_cyrillic {
            return ScriptClass::CyrillicOnly;
        }
        return ScriptClass::OtherSingleScript(other_script_name.unwrap_or_default());
    }

    if script_count == 2 {
        if has_latin && has_arabic {
            return ScriptClass::LatinArabicMixed;
        }
        if has_latin && has_cyrillic {
            return ScriptClass::LatinCyrillicMixed;
        }
    }

    ScriptClass::OtherMixedScript
}

// ─── Shape classification ───────────────────────────────────────────────────

/// Shape flags for a word.
#[derive(Debug, Clone, Default)]
pub struct ShapeFlags {
    pub is_punctuation_only: bool,
    pub is_symbol_only: bool,
    pub is_digit_only: bool,
    pub has_no_letters: bool,
    pub is_uppercase_only: bool,
    pub is_title_case: bool,
    pub is_mixed_case: bool,
    pub contains_digits: bool,
    pub contains_hyphen: bool,
    pub contains_apostrophe: bool,
    pub is_multiword: bool,
    pub is_very_short: bool,   // grapheme count <= 1
    pub is_very_long_25: bool, // grapheme count > 25
    pub is_very_long_40: bool, // grapheme count > 40
    pub unicode_scalar_length: usize,
    pub grapheme_length: usize,
}

/// Classifies the shape of a word using precise Unicode categories.
pub fn classify_shape(original_word: &str, normalized: &str) -> ShapeFlags {
    let grapheme_count = normalized.graphemes(true).count();
    let scalar_count = normalized.chars().count();

    let non_ws_chars: Vec<char> = normalized.chars().filter(|c| !c.is_whitespace()).collect();

    // Punctuation-only: every non-WS char is P*, at least one
    let is_punctuation_only =
        !non_ws_chars.is_empty() && non_ws_chars.iter().all(|c| is_unicode_punctuation(*c));

    // Symbol-only: every non-WS char is S*, at least one
    let is_symbol_only = !is_punctuation_only
        && !non_ws_chars.is_empty()
        && non_ws_chars.iter().all(|c| is_unicode_symbol(*c));

    // Digit-only: every non-WS char is N*, at least one
    let is_digit_only = !is_punctuation_only
        && !is_symbol_only
        && !non_ws_chars.is_empty()
        && non_ws_chars.iter().all(|c| is_unicode_number(*c));

    // No letters
    let has_no_letters = !non_ws_chars.iter().any(|c| is_unicode_letter(*c));

    // Casing analysis on the ORIGINAL word (preserving case info)
    let cased_chars: Vec<char> = original_word
        .chars()
        .filter(|c| c.is_uppercase() || c.is_lowercase())
        .collect();
    let has_uppercase = cased_chars.iter().any(|c| c.is_uppercase());
    let has_lowercase = cased_chars.iter().any(|c| c.is_lowercase());
    let has_cased = !cased_chars.is_empty();

    // Uppercase-only: at least one cased letter, every cased letter uppercase, no lowercase
    let is_uppercase_only = has_cased && has_uppercase && !has_lowercase;

    // Title case: first letter uppercase, rest lowercase, at least 2 letters
    let letters: Vec<char> = original_word
        .chars()
        .filter(|c| is_unicode_letter(*c))
        .collect();
    let is_title_case = letters.len() >= 2
        && letters[0].is_uppercase()
        && letters[1..].iter().all(|c| !c.is_uppercase());

    // Mixed case: has both uppercase and lowercase cased characters, not title case
    let is_mixed_case = has_uppercase && has_lowercase && !is_title_case;

    // Contains digits: at least one N* and at least one L*
    let contains_digits = non_ws_chars.iter().any(|c| is_unicode_number(*c))
        && non_ws_chars.iter().any(|c| is_unicode_letter(*c));

    let contains_hyphen = normalized.contains('-');
    let contains_apostrophe = normalized.contains('\'') || normalized.contains('\u{2019}');
    let is_multiword = normalized.contains(' ');

    ShapeFlags {
        is_punctuation_only,
        is_symbol_only,
        is_digit_only,
        has_no_letters,
        is_uppercase_only,
        is_title_case,
        is_mixed_case,
        contains_digits,
        contains_hyphen,
        contains_apostrophe,
        is_multiword,
        is_very_short: grapheme_count <= 1,
        is_very_long_25: grapheme_count > 25,
        is_very_long_40: grapheme_count > 40,
        unicode_scalar_length: scalar_count,
        grapheme_length: grapheme_count,
    }
}

// ─── Structural / raw-line checks ───────────────────────────────────────────

/// Findings from raw-line structural analysis.
#[derive(Debug, Clone)]
pub struct RawLineFindings {
    pub has_leading_whitespace: bool,
    pub has_trailing_whitespace: bool,
    pub has_tab: bool,
    pub has_control_char: bool,
    pub has_unexpected_cr: bool,
    pub has_null_byte: bool,
    pub has_replacement_char: bool,
}

/// Checks a raw source line for structural anomalies. The line should be
/// exactly as read (after line terminator removal but before trimming).
/// A normal `\r` from CRLF decoding is already stripped by BufRead.
pub fn check_raw_line(raw_line: &str) -> RawLineFindings {
    let has_leading = raw_line.len() != raw_line.trim_start().len() && !raw_line.is_empty();
    let has_trailing = raw_line.len() != raw_line.trim_end().len() && !raw_line.is_empty();

    let has_tab = raw_line.contains('\t');
    let has_null_byte = raw_line.contains('\0');
    let has_replacement_char = raw_line.contains('\u{FFFD}');

    // An embedded \r is suspicious (BufRead already strips trailing \r\n)
    let has_unexpected_cr = raw_line.contains('\r');

    let has_control_char = raw_line
        .chars()
        .any(|ch| is_unicode_control(ch) && ch != '\t' && ch != '\n' && ch != '\r' && ch != '\0');

    RawLineFindings {
        has_leading_whitespace: has_leading,
        has_trailing_whitespace: has_trailing,
        has_tab,
        has_control_char,
        has_unexpected_cr,
        has_null_byte,
        has_replacement_char,
    }
}

/// Returns the possible-proper-noun flag. Original word starts uppercase,
/// remaining letters all lowercase, at least 2 letters total.
pub fn is_possible_proper_noun(original_word: &str) -> bool {
    let letters: Vec<char> = original_word
        .chars()
        .filter(|c| is_unicode_letter(*c))
        .collect();
    if letters.len() < 2 {
        return false;
    }
    letters[0].is_uppercase() && letters[1..].iter().all(|c| c.is_lowercase())
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_punctuation_only() {
        assert!(is_unicode_punctuation('!'));
        assert!(is_unicode_punctuation('.'));
        assert!(is_unicode_punctuation(','));
        assert!(!is_unicode_punctuation('$')); // $ is Sc (currency symbol)
        assert!(!is_unicode_punctuation('5')); // digit
        assert!(!is_unicode_punctuation('a')); // letter

        let shape = classify_shape("!", "!");
        assert!(shape.is_punctuation_only);
        assert!(!shape.is_symbol_only);
        assert!(!shape.is_digit_only);
    }

    #[test]
    fn test_symbol_only() {
        assert!(is_unicode_symbol('$'));
        assert!(is_unicode_symbol('€'));
        assert!(is_unicode_symbol('+'));

        let shape = classify_shape("$", "$");
        assert!(!shape.is_punctuation_only);
        assert!(shape.is_symbol_only);
    }

    #[test]
    fn test_digit_only() {
        let shape = classify_shape("123", "123");
        assert!(shape.is_digit_only);
        assert!(!shape.is_punctuation_only);
        assert!(!shape.is_symbol_only);
    }

    #[test]
    fn test_no_letters() {
        let shape = classify_shape("123!", "123!");
        assert!(shape.has_no_letters);
        assert!(!shape.is_digit_only); // has punctuation too
    }

    #[test]
    fn test_uppercase_only_requires_cased_letter() {
        // Punctuation-only should NOT be classified as uppercase-only
        let shape = classify_shape("!!!", "!!!");
        assert!(!shape.is_uppercase_only);

        // Real uppercase-only
        let shape = classify_shape("ABC", "abc");
        assert!(shape.is_uppercase_only);
    }

    #[test]
    fn test_title_case() {
        let shape = classify_shape("Rojbaş", "rojbaş");
        assert!(shape.is_title_case);
        assert!(!shape.is_uppercase_only);
    }

    #[test]
    fn test_script_latin_only() {
        assert_eq!(classify_script("rojbaş"), ScriptClass::LatinOnly);
    }

    #[test]
    fn test_script_no_scripted_letters() {
        assert_eq!(classify_script("123!"), ScriptClass::NoScriptedLetters);
    }

    #[test]
    fn test_digits_not_mixed_script() {
        // "abc123" has Latin letters and digits, but digits are Common script
        assert_eq!(classify_script("abc123"), ScriptClass::LatinOnly);
    }

    #[test]
    fn test_common_punctuation_between_latin() {
        // Hyphen and apostrophe are Common script, not mixed
        assert_eq!(classify_script("xwe-xwe"), ScriptClass::LatinOnly);
    }

    #[test]
    fn test_grapheme_vs_scalar_length() {
        // é as e + combining acute (2 scalars, 1 grapheme after visual rendering)
        let shape = classify_shape("e\u{0301}", "e\u{0301}");
        assert_eq!(shape.unicode_scalar_length, 2);
        assert_eq!(shape.grapheme_length, 1);
    }

    #[test]
    fn test_raw_line_clean() {
        let findings = check_raw_line("rojbaş/AN po:noun");
        assert!(!findings.has_leading_whitespace);
        assert!(!findings.has_trailing_whitespace);
        assert!(!findings.has_tab);
        assert!(!findings.has_control_char);
    }

    #[test]
    fn test_raw_line_trailing_space() {
        let findings = check_raw_line("rojbaş/AN ");
        assert!(findings.has_trailing_whitespace);
    }

    #[test]
    fn test_raw_line_embedded_cr() {
        let findings = check_raw_line("rojbaş\rAN");
        assert!(findings.has_unexpected_cr);
    }
}
