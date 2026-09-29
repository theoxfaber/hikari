//! SHA-256 content-hash cache: identical subtrees skip repeat work.

use sha2::{Digest, Sha256};
use std::collections::HashMap;

use crate::Node;

/// Hash opaque bytes, hex-encoded.
#[must_use]
pub fn hash_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Stable hash of a node tree + viewport. Path-independent.
#[must_use]
pub fn hash_node(node: &Node, viewport_w: u32, viewport_h: u32) -> String {
    let mut h = Sha256::new();
    let json = serde_json::to_vec(node).unwrap_or_default();
    h.update(&json);
    h.update(viewport_w.to_le_bytes());
    h.update(viewport_h.to_le_bytes());
    hex::encode(h.finalize())
}

/// Tiny LRU-less cache keyed by content hash. Caller decides eviction.
#[derive(Debug, Default)]
pub struct HashCache<T> {
    inner: HashMap<String, T>,
    /// Cache hits since creation.
    pub hits: u64,
    /// Cache misses since creation.
    pub misses: u64,
}

impl<T> HashCache<T> {
    /// Empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
            hits: 0,
            misses: 0,
        }
    }

    /// Get by hash, counting hits/misses.
    #[must_use]
    pub fn get(&mut self, key: &str) -> Option<&T> {
        if let Some(v) = self.inner.get(key) {
            self.hits += 1;
            Some(v)
        } else {
            self.misses += 1;
            None
        }
    }

    /// Insert by hash.
    pub fn insert(&mut self, key: String, value: T) {
        self.inner.insert(key, value);
    }

    /// Number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// True when empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Node, Style};

    #[test]
    fn hash_is_stable_and_path_independent() {
        let a = Node::banner(1200.0, 630.0, "#111111", "Hi", 64.0, "#ffffff");
        let b = Node::banner(1200.0, 630.0, "#111111", "Hi", 64.0, "#ffffff");
        assert_eq!(hash_node(&a, 1200, 630), hash_node(&b, 1200, 630));
        assert_ne!(hash_node(&a, 1200, 630), hash_node(&a, 800, 600));
        let _ = Style::new();
    }

    #[test]
    fn cache_counts() {
        let mut c: HashCache<Vec<u8>> = HashCache::new();
        assert!(c.get("k").is_none());
        c.insert("k".to_owned(), vec![1]);
        assert!(c.get("k").is_some());
        assert_eq!((c.hits, c.misses), (1, 1));
    }
}
