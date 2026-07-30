use unicode_normalization::UnicodeNormalization;

/// Normalizes Kurmancî text into standard NFC Unicode representation and converts to canonical lowercase.
/// Preserves distinct Kurmancî letters (`ç`, `ê`, `î`, `ş`, `û`).
pub fn normalize_text(text: &str) -> String {
    let clean: String = text
        .chars()
        .filter(|ch| !ch.is_control() && *ch != '\u{200B}' && *ch != '\u{FEFF}')
        .collect();

    clean.nfc().collect::<String>().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kurmanci_letter_preservation() {
        assert_eq!(normalize_text("ROJBAŞ"), "rojbaş");
        assert_eq!(normalize_text("BIJÎ"), "bijî");
        assert_eq!(normalize_text("PIRTÛK"), "pirtûk");
        assert_eq!(normalize_text("ÇAV"), "çav");
        assert_eq!(normalize_text("ÊDÎ"), "êdî");
    }

    #[test]
    fn test_zero_width_filtering() {
        let dirty = "roj\u{200B}baş";
        assert_eq!(normalize_text(dirty), "rojbaş");
    }
}
