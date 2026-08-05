use std::collections::HashMap;

#[derive(Default, Debug, Clone)]
pub struct TrieNode {
    pub children: HashMap<char, TrieNode>,
    pub is_terminal: bool,
    pub word: Option<String>,
    pub frequency: u64,
}

#[derive(Default, Debug, Clone)]
pub struct Trie {
    root: TrieNode,
}

impl Trie {
    pub fn new() -> Self {
        Self {
            root: TrieNode::default(),
        }
    }

    /// Inserts a word and its frequency into the Trie.
    pub fn insert(&mut self, word: &str, frequency: u64) {
        let mut current = &mut self.root;

        for ch in word.chars() {
            current = current.children.entry(ch).or_default();
        }

        current.is_terminal = true;
        current.word = Some(word.to_string());
        current.frequency = frequency;
    }

    /// Checks if an exact word exists in the Trie.
    pub fn contains(&self, word: &str) -> bool {
        let mut current = &self.root;

        for ch in word.chars() {
            if let Some(next) = current.children.get(&ch) {
                current = next;
            } else {
                return false;
            }
        }

        current.is_terminal
    }

    /// Finds all words starting with the given prefix. Returns a vector of tuples `(word, frequency)`.
    pub fn find_by_prefix(&self, prefix: &str) -> Vec<(String, u64)> {
        let mut current = &self.root;

        for ch in prefix.chars() {
            if let Some(next) = current.children.get(&ch) {
                current = next;
            } else {
                return Vec::new();
            }
        }

        let mut results = Vec::new();
        Self::collect_words(current, &mut results);
        results
    }

    fn collect_words(node: &TrieNode, results: &mut Vec<(String, u64)>) {
        if node.is_terminal {
            if let Some(ref word) = node.word {
                results.push((word.clone(), node.frequency));
            }
        }

        for child in node.children.values() {
            Self::collect_words(child, results);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trie_prefix() {
        let mut trie = Trie::new();
        trie.insert("roj", 68100);
        trie.insert("roja", 72150);
        trie.insert("rojbaş", 84231);
        trie.insert("bijî", 91200);

        assert!(trie.contains("rojbaş"));
        assert!(!trie.contains("rojba"));

        let completions = trie.find_by_prefix("roj");
        assert_eq!(completions.len(), 3);
    }
}
