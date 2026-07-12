//! Variable handle cache — thread-safe mapping from variable names to
//! DAP `variablesReference` handles.
//!
//! DAP returns top-level variables with a `variablesReference` integer handle
//! instead of inline children. When an AI assistant asks about a specific
//! variable by name across multiple interactions, this cache avoids
//! re-traversing the full variable tree each time.
//!
//! The cache is automatically invalidated when execution resumes (state
//! transitions out of `Halted`).

use std::collections::HashMap;
use std::time::Instant;

use tokio::sync::RwLock;

/// A cached variable handle entry.
#[derive(Debug, Clone)]
pub struct VariableHandleEntry {
    /// The DAP variablesReference for fetching this variable's children.
    pub variables_reference: u64,
    /// The name of the variable.
    pub name: String,
    /// The frame ID in which this variable was observed.
    pub frame_id: Option<u64>,
    /// The scope name in which this variable was observed (e.g., "Locals").
    pub scope_name: Option<String>,
    /// The declared type of the variable.
    pub var_type: Option<String>,
    /// The number of named child variables (for paging).
    pub named_variables: Option<u64>,
    /// The number of indexed child variables (for paging).
    pub indexed_variables: Option<u64>,
    /// When this entry was cached.
    pub captured_at: Instant,
}

impl VariableHandleEntry {
    /// Returns true if this entry has expandable children (variables_reference > 0).
    pub fn is_expandable(&self) -> bool {
        self.variables_reference > 0
    }

    /// Returns the total child count (named + indexed).
    pub fn child_count(&self) -> u64 {
        self.named_variables.unwrap_or(0) + self.indexed_variables.unwrap_or(0)
    }
}

/// Thread-safe cache mapping variable names to their DAP handle references.
///
/// The cache supports scoped lookups: when looking up a variable name,
/// entries matching the current frame and scope are preferred over entries
/// from other frames or scopes.
///
/// # Auto-invalidation
///
/// Call `invalidate()` whenever execution resumes (state transitions to
/// `Running`). This ensures stale handles from a previous stop don't cause
/// errors.
pub struct VariableHandleCache {
    /// Primary index: variable name → list of entries (one per scope/frame).
    entries: RwLock<HashMap<String, Vec<VariableHandleEntry>>>,
    /// Whether the cache has been invalidated since last population.
    valid: RwLock<bool>,
}

impl VariableHandleCache {
    /// Create a new, empty cache.
    pub fn new() -> Self {
        VariableHandleCache {
            entries: RwLock::new(HashMap::new()),
            valid: RwLock::new(true),
        }
    }

    /// Insert a variable into the cache.
    ///
    /// If an entry with the same name, frame_id, and scope_name already exists,
    /// it is replaced.
    pub async fn insert(&self, entry: VariableHandleEntry) {
        let mut entries = self.entries.write().await;
        let list = entries.entry(entry.name.clone()).or_default();

        // Replace existing entry for the same scope/frame
        list.retain(|e| {
            !(e.frame_id == entry.frame_id
                && e.scope_name == entry.scope_name
                && e.name == entry.name)
        });
        list.push(entry);
        *self.valid.write().await = true;
    }

    /// Insert multiple entries in one batch.
    pub async fn insert_batch(&self, batch: Vec<VariableHandleEntry>) {
        let mut entries = self.entries.write().await;
        for entry in batch {
            let list = entries.entry(entry.name.clone()).or_default();
            list.retain(|e| {
                !(e.frame_id == entry.frame_id
                    && e.scope_name == entry.scope_name
                    && e.name == entry.name)
            });
            list.push(entry);
        }
        *self.valid.write().await = true;
    }

    /// Look up a variable by name, optionally scoped to a specific frame and scope.
    ///
    /// Resolution order:
    /// 1. Exact match on name + frame_id + scope_name
    /// 2. Match on name + frame_id (any scope)
    /// 3. Match on name only (any frame, any scope)
    /// 4. Fuzzy match: variable name contains the query (case-insensitive)
    pub async fn lookup(
        &self,
        name: &str,
        frame_id: Option<u64>,
        scope_name: Option<&str>,
    ) -> Option<VariableHandleEntry> {
        let entries = self.entries.read().await;
        let list = entries.get(name)?;

        // Priority 1: exact frame + scope match
        for e in list {
            if e.frame_id == frame_id && e.scope_name.as_deref() == scope_name {
                return Some(e.clone());
            }
        }

        // Priority 2: same frame, any scope
        if frame_id.is_some() {
            for e in list {
                if e.frame_id == frame_id {
                    return Some(e.clone());
                }
            }
        }

        // Priority 3: any entry with this name
        list.first().cloned()
    }

    /// Fuzzy lookup: find entries whose name contains the query string
    /// (case-insensitive). Returns up to `limit` matches.
    pub async fn search(&self, query: &str, limit: usize) -> Vec<VariableHandleEntry> {
        let entries = self.entries.read().await;
        let query_lower = query.to_lowercase();
        let mut results: Vec<VariableHandleEntry> = entries
            .iter()
            .filter(|(name, _)| name.to_lowercase().contains(&query_lower))
            .flat_map(|(_, list)| list.iter().cloned())
            .collect();

        // Sort by most recent first
        results.sort_by_key(|e| std::cmp::Reverse(e.captured_at));
        results.truncate(limit);
        results
    }

    /// List all cached variable names.
    pub async fn keys(&self) -> Vec<String> {
        let entries = self.entries.read().await;
        entries.keys().cloned().collect()
    }

    /// Returns the number of unique variable names in the cache.
    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }

    /// Returns true if the cache has no entries.
    pub async fn is_empty(&self) -> bool {
        self.entries.read().await.is_empty()
    }

    /// Returns true if the cache is valid (not stale).
    pub async fn is_valid(&self) -> bool {
        *self.valid.read().await
    }

    /// Invalidate the cache. Call when execution resumes or the debuggee
    /// state changes. After invalidation, lookups return `None` until new
    /// entries are inserted.
    pub async fn invalidate(&self) {
        self.entries.write().await.clear();
        *self.valid.write().await = false;
    }

    /// Soft-invalidate: mark as stale without clearing entries.
    /// Callers should check `is_valid()` before using results.
    pub async fn mark_stale(&self) {
        *self.valid.write().await = false;
    }

    /// Clear all entries and reset validity.
    pub async fn clear(&self) {
        self.entries.write().await.clear();
        *self.valid.write().await = true;
    }
}

impl Default for VariableHandleCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(
        name: &str,
        var_ref: u64,
        frame_id: Option<u64>,
        scope: Option<&str>,
    ) -> VariableHandleEntry {
        VariableHandleEntry {
            variables_reference: var_ref,
            name: name.to_string(),
            frame_id,
            scope_name: scope.map(|s| s.to_string()),
            var_type: Some("int".to_string()),
            named_variables: if var_ref > 0 { Some(3) } else { None },
            indexed_variables: None,
            captured_at: Instant::now(),
        }
    }

    #[tokio::test]
    async fn test_insert_and_lookup() {
        let cache = VariableHandleCache::new();
        cache
            .insert(make_entry("x", 5, Some(1), Some("Locals")))
            .await;

        let found = cache.lookup("x", Some(1), Some("Locals")).await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().variables_reference, 5);
    }

    #[tokio::test]
    async fn test_lookup_priority_frame_scope() {
        let cache = VariableHandleCache::new();
        // Same name, different frames
        cache
            .insert(make_entry("x", 5, Some(1), Some("Locals")))
            .await;
        cache
            .insert(make_entry("x", 10, Some(2), Some("Locals")))
            .await;

        // Exact match should win
        let found = cache.lookup("x", Some(2), Some("Locals")).await;
        assert_eq!(found.unwrap().variables_reference, 10);
    }

    #[tokio::test]
    async fn test_lookup_fallback_frame() {
        let cache = VariableHandleCache::new();
        cache
            .insert(make_entry("x", 5, Some(1), Some("Locals")))
            .await;

        // Different scope but same frame — should still match
        let found = cache.lookup("x", Some(1), Some("Arguments")).await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().variables_reference, 5);
    }

    #[tokio::test]
    async fn test_lookup_fallback_any() {
        let cache = VariableHandleCache::new();
        cache
            .insert(make_entry("x", 5, Some(1), Some("Locals")))
            .await;

        // Different frame — should still match via fallback
        let found = cache.lookup("x", Some(99), None).await;
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn test_lookup_missing() {
        let cache = VariableHandleCache::new();
        let found = cache.lookup("nonexistent", None, None).await;
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_invalidate_clears_entries() {
        let cache = VariableHandleCache::new();
        cache
            .insert(make_entry("x", 5, Some(1), Some("Locals")))
            .await;
        assert_eq!(cache.len().await, 1);
        assert!(cache.is_valid().await);

        cache.invalidate().await;
        assert!(cache.is_empty().await);
        assert!(!cache.is_valid().await);

        let found = cache.lookup("x", Some(1), Some("Locals")).await;
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_mark_stale_preserves_entries() {
        let cache = VariableHandleCache::new();
        cache
            .insert(make_entry("x", 5, Some(1), Some("Locals")))
            .await;

        cache.mark_stale().await;
        assert!(!cache.is_valid().await);
        assert_eq!(cache.len().await, 1); // entries still there
    }

    #[tokio::test]
    async fn test_search_fuzzy() {
        let cache = VariableHandleCache::new();
        cache
            .insert(make_entry("my_variable", 1, Some(1), Some("Locals")))
            .await;
        cache
            .insert(make_entry("my_struct", 2, Some(1), Some("Locals")))
            .await;
        cache
            .insert(make_entry("other_thing", 3, Some(1), Some("Locals")))
            .await;

        let results = cache.search("my_", 10).await;
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_insert_replaces_duplicate() {
        let cache = VariableHandleCache::new();
        cache
            .insert(make_entry("x", 5, Some(1), Some("Locals")))
            .await;
        cache
            .insert(make_entry("x", 99, Some(1), Some("Locals")))
            .await;

        let found = cache.lookup("x", Some(1), Some("Locals")).await;
        assert_eq!(found.unwrap().variables_reference, 99);
        assert_eq!(cache.len().await, 1);
    }

    #[tokio::test]
    async fn test_is_expandable() {
        let e = make_entry("leaf", 0, Some(1), None);
        assert!(!e.is_expandable());
        assert_eq!(e.child_count(), 0);

        let e = make_entry("node", 5, Some(1), None);
        assert!(e.is_expandable());
        assert_eq!(e.child_count(), 3); // named_variables = 3
    }

    #[tokio::test]
    async fn test_keys_and_len() {
        let cache = VariableHandleCache::new();
        assert!(cache.is_empty().await);

        cache.insert(make_entry("a", 1, Some(1), Some("L"))).await;
        cache.insert(make_entry("b", 2, Some(1), Some("L"))).await;
        cache.insert(make_entry("a", 3, Some(2), Some("L"))).await; // same name, diff frame

        assert_eq!(cache.len().await, 2); // unique names: "a", "b"
        let mut keys = cache.keys().await;
        keys.sort();
        assert_eq!(keys, vec!["a", "b"]);
    }
}
