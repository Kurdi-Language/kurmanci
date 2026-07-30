/// Returns substitution penalty for two characters.
/// Special Kurmancî diacritic pairs (`i ↔ î`, `u ↔ û`, `s ↔ ş`, `c ↔ ç`, `e ↔ ê`) have a cost of 0.25.
/// Physically adjacent QWERTY/QWERTZ key pairs have a cost of 0.75.
pub fn substitution_cost(a: char, b: char) -> f64 {
    if a == b {
        return 0.0;
    }

    let is_diacritic_pair = matches!(
        (a, b),
        ('i', 'î')
            | ('î', 'i')
            | ('u', 'û')
            | ('û', 'u')
            | ('s', 'ş')
            | ('ş', 's')
            | ('c', 'ç')
            | ('ç', 'c')
            | ('e', 'ê')
            | ('ê', 'e')
    );

    if is_diacritic_pair {
        return 0.25;
    }

    let is_adjacent_key = matches!(
        (a, b),
        ('r', 't')
            | ('t', 'r')
            | ('r', 'e')
            | ('e', 'r')
            | ('a', 's')
            | ('s', 'a')
            | ('a', 'q')
            | ('q', 'a')
            | ('a', 'z')
            | ('z', 'a')
            | ('z', 'x')
            | ('x', 'z')
            | ('x', 'c')
            | ('c', 'x')
            | ('c', 'v')
            | ('v', 'c')
            | ('v', 'b')
            | ('b', 'v')
            | ('b', 'n')
            | ('n', 'b')
            | ('n', 'm')
            | ('m', 'n')
    );

    if is_adjacent_key {
        0.75
    } else {
        1.0
    }
}

/// Calculates weighted Damerau-Levenshtein edit distance between two strings.
/// Supports insertions, deletions, substitutions (with diacritic and keyboard adjacency penalties), and transpositions.
#[allow(clippy::needless_range_loop)]
pub fn weighted_damerau_levenshtein(s1: &str, s2: &str) -> f64 {
    let chars1: Vec<char> = s1.chars().collect();
    let chars2: Vec<char> = s2.chars().collect();

    let len1 = chars1.len();
    let len2 = chars2.len();

    if len1 == 0 {
        return len2 as f64;
    }
    if len2 == 0 {
        return len1 as f64;
    }

    let mut dp = vec![vec![0.0; len2 + 1]; len1 + 1];

    for i in 0..=len1 {
        dp[i][0] = i as f64;
    }
    for j in 0..=len2 {
        dp[0][j] = j as f64;
    }

    for i in 1..=len1 {
        for j in 1..=len2 {
            let cost = substitution_cost(chars1[i - 1], chars2[j - 1]);

            let deletion = dp[i - 1][j] + 1.0;
            let insertion = dp[i][j - 1] + 1.0;
            let substitution = dp[i - 1][j - 1] + cost;

            let mut min_val = deletion.min(insertion).min(substitution);

            // Check adjacent transposition
            if i > 1 && j > 1 && chars1[i - 1] == chars2[j - 2] && chars1[i - 2] == chars2[j - 1] {
                let transposition = dp[i - 2][j - 2] + 0.75;
                min_val = min_val.min(transposition);
            }

            dp[i][j] = min_val;
        }
    }

    dp[len1][len2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diacritic_costs() {
        assert_eq!(weighted_damerau_levenshtein("biji", "bijî"), 0.25);
        assert_eq!(weighted_damerau_levenshtein("rojbas", "rojbaş"), 0.25);
    }

    #[test]
    fn test_keyboard_adjacency() {
        assert_eq!(substitution_cost('r', 't'), 0.75);
        assert_eq!(substitution_cost('a', 's'), 0.75);
    }

    #[test]
    fn test_transposition() {
        assert_eq!(weighted_damerau_levenshtein("rjobaş", "rojbaş"), 0.75);
    }
}
