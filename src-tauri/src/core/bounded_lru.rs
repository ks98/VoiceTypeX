// SPDX-License-Identifier: GPL-3.0-or-later
//! A tiny dependency-free bounded LRU map.
//!
//! Used for the per-override engine caches (`extra_transcribers`,
//! `extra_llm_processors`): each cached value backs a multi-GB model, so
//! without a bound a session that switches across many override slots
//! accumulates unbounded model memory (issue #31). Capping the cache at a
//! handful of entries and evicting the least-recently-used one keeps the
//! resident model count bounded.
//!
//! The cache is small (single-digit capacity), so the recency order is a
//! plain `Vec<K>` — O(capacity) per access, which is cheaper than the
//! bookkeeping of a linked-list LRU and needs no extra crate.

use std::collections::HashMap;
use std::hash::Hash;

/// A `HashMap` capped at `capacity` entries with least-recently-used
/// eviction. `get` and `insert` count as uses and move the key to the
/// most-recently-used end.
pub struct BoundedLru<K, V> {
    map: HashMap<K, V>,
    /// Keys ordered least-recently-used (front) to most-recently-used
    /// (back). Always holds exactly the keys present in `map`.
    order: Vec<K>,
    capacity: usize,
}

impl<K: Eq + Hash + Clone, V> BoundedLru<K, V> {
    /// `capacity` must be > 0; a zero-capacity cache could never hold the
    /// entry it was just asked to insert.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "BoundedLru capacity must be > 0");
        Self {
            map: HashMap::new(),
            order: Vec::with_capacity(capacity),
            capacity,
        }
    }

    /// Returns the value for `key` and marks it most-recently-used.
    pub fn get(&mut self, key: &K) -> Option<&V> {
        if self.map.contains_key(key) {
            self.touch(key);
            self.map.get(key)
        } else {
            None
        }
    }

    /// Inserts `key`/`value`, marks it most-recently-used, and evicts the
    /// least-recently-used entry if the cache is over capacity. Re-inserting
    /// an existing key overwrites its value and refreshes its recency.
    pub fn insert(&mut self, key: K, value: V) {
        if self.map.insert(key.clone(), value).is_some() {
            self.touch(&key);
        } else {
            self.order.push(key);
            if self.order.len() > self.capacity {
                let evicted = self.order.remove(0);
                self.map.remove(&evicted);
                // The evicted value is only dropped here; a pipeline run
                // that already cloned the `Arc` keeps its model alive until
                // it finishes (issue #31 acceptance: eviction must not break
                // an in-flight mode).
            }
        }
    }

    /// Moves an existing key to the most-recently-used end.
    fn touch(&mut self, key: &K) {
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            let k = self.order.remove(pos);
            self.order.push(k);
        }
    }

    #[cfg(test)]
    fn entry_count(&self) -> usize {
        debug_assert_eq!(self.map.len(), self.order.len());
        self.map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_least_recently_used_over_capacity() {
        let mut lru: BoundedLru<&str, i32> = BoundedLru::new(2);
        lru.insert("a", 1);
        lru.insert("b", 2);
        // Inserting a third distinct key evicts the LRU ("a").
        lru.insert("c", 3);

        assert_eq!(lru.entry_count(), 2);
        assert!(lru.get(&"a").is_none(), "a should have been evicted");
        assert_eq!(lru.get(&"b"), Some(&2));
        assert_eq!(lru.get(&"c"), Some(&3));
    }

    #[test]
    fn get_refreshes_recency() {
        let mut lru: BoundedLru<&str, i32> = BoundedLru::new(2);
        lru.insert("a", 1);
        lru.insert("b", 2);
        // Touch "a" so "b" becomes the LRU.
        assert_eq!(lru.get(&"a"), Some(&1));
        lru.insert("c", 3);

        assert_eq!(lru.entry_count(), 2);
        assert!(lru.get(&"b").is_none(), "b should have been evicted");
        assert_eq!(lru.get(&"a"), Some(&1));
        assert_eq!(lru.get(&"c"), Some(&3));
    }

    #[test]
    fn reinsert_overwrites_without_growing() {
        let mut lru: BoundedLru<&str, i32> = BoundedLru::new(2);
        lru.insert("a", 1);
        lru.insert("a", 99);
        assert_eq!(lru.entry_count(), 1);
        assert_eq!(lru.get(&"a"), Some(&99));
    }
}
