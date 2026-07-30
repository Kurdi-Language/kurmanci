use unicode_normalization::UnicodeNormalization;

/// Normalizes text into standard NFC Unicode representation and converts to lowercase.
pub fn normalize(text: &str) -> String {
    text.nfc().collect::<String>().to_lowercase()
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
