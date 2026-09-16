use unicode_normalization::UnicodeNormalization;

/// Canonical input normalization, identical to the repository's `normalize_text` used to
/// build packs and review identities: Unicode control characters, U+200B (zero-width space)
/// and U+FEFF (byte order mark) are removed, then the text is NFC-normalized and lower-cased.
/// Distinct Kurmancî letters (`ç ê î ş û`) are preserved; ordinary whitespace and NBSP are
/// not removed.
pub fn normalize(text: &str) -> String {
    let clean: String = text
        .chars()
        .filter(|ch| !ch.is_control() && *ch != '\u{200B}' && *ch != '\u{FEFF}')
        .collect();
    clean.nfc().collect::<String>().to_lowercase()
}

/// Converts Kurmancî diacritics into ASCII equivalent base letters for indexing and fallback matching.
/// `î -> i`, `û -> u`, `ş -> s`, `ç -> c`, `ê -> e`
pub fn strip_diacritics(text: &str) -> String {
    normalize(text)
        .chars()
        .map(|ch| match ch {
            'î' => 'i',
            'û' => 'u',
            'ş' => 's',
            'ç' => 'c',
            'ê' => 'e',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalization() {
        assert_eq!(normalize("ROJBAŞ"), "rojbaş");
        assert_eq!(normalize("BIJÎ"), "bijî");
        assert_eq!(normalize("PIRTÛK"), "pirtûk");
        assert_eq!(normalize("ÇAV"), "çav");
        assert_eq!(normalize("ÊDÎ"), "êdî");
    }

    #[test]
    fn test_canonical_cleaning_matches_repository_rule() {
        // Same rule as data-builder's normalize_text: controls, U+200B and U+FEFF removed.
        assert_eq!(normalize("roj\u{200B}baş"), "rojbaş");
        assert_eq!(normalize("\u{FEFF}rojbaş"), "rojbaş");
        assert_eq!(normalize("roj\u{0001}baş\u{0000}"), "rojbaş");
        assert_eq!(normalize("rojbaş\t"), "rojbaş");
        // Ordinary whitespace and NBSP are not part of the cleaning rule.
        assert_eq!(normalize(" rojbaş"), " rojbaş");
        assert_eq!(normalize("rojbaş\u{00A0}"), "rojbaş\u{00A0}");
        assert_eq!(normalize("rojbas\u{0327}"), "rojbaş");
    }

    #[test]
    fn test_strip_diacritics() {
        assert_eq!(strip_diacritics("rojbaş"), "rojbas");
        assert_eq!(strip_diacritics("bijî"), "biji");
        assert_eq!(strip_diacritics("pirtûk"), "pirtuk");
        assert_eq!(strip_diacritics("çav"), "cav");
        assert_eq!(strip_diacritics("êdî"), "edi");
    }
}
