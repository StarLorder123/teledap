//! Breakpoint cache — client-side tracking of source and function breakpoints.
//!
//! DAP does not define a request to list existing breakpoints, so TeleDAP
//! remembers the breakpoints it has set and optionally refreshes them from
//! `breakpoint` events.

use std::collections::HashMap;

use dap_types::types::{Breakpoint, FunctionBreakpoint, SourceBreakpoint};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Discriminator for the kind of breakpoint stored in the cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BreakpointKind {
    Source,
    Function,
}

/// A breakpoint entry suitable for returning from the `list_breakpoints` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedBreakpoint {
    /// Breakpoint ID assigned by the debug adapter (if verified).
    pub id: Option<u64>,
    /// Type discriminator: "source" or "function".
    pub kind: BreakpointKind,
    /// Source path (reverse-mapped to alias form when listed if possible).
    pub source_path: Option<String>,
    /// Line number (1-based).
    pub line: Option<u64>,
    /// Column number (if available).
    pub column: Option<u64>,
    /// Function name (for function breakpoints).
    pub function_name: Option<String>,
    /// Condition expression (if set).
    pub condition: Option<String>,
    /// Hit condition expression (if set).
    pub hit_condition: Option<String>,
    /// Whether the breakpoint was successfully verified by the adapter.
    pub verified: bool,
    /// Adapter message about the breakpoint state.
    pub message: Option<String>,
}

impl CachedBreakpoint {
    fn from_source(source_path: &str, request: &SourceBreakpoint, response: &Breakpoint) -> Self {
        CachedBreakpoint {
            id: response.id,
            kind: BreakpointKind::Source,
            source_path: Some(source_path.to_string()),
            line: response.line.or(Some(request.line)),
            column: response.column.or(request.column),
            function_name: None,
            condition: request.condition.clone(),
            hit_condition: request.hit_condition.clone(),
            verified: response.verified,
            message: response.message.clone(),
        }
    }

    fn from_function(request: &FunctionBreakpoint, response: &Breakpoint) -> Self {
        CachedBreakpoint {
            id: response.id,
            kind: BreakpointKind::Function,
            source_path: response.source.as_ref().and_then(|s| s.path.clone()),
            line: response.line,
            column: response.column,
            function_name: Some(request.name.clone()),
            condition: request.condition.clone(),
            hit_condition: request.hit_condition.clone(),
            verified: response.verified,
            message: response.message.clone(),
        }
    }
}

/// Thread-safe cache of all breakpoints set during the session.
pub struct BreakpointCache {
    /// Source breakpoints indexed by resolved absolute source path.
    /// A new `setBreakpoints` call for a source replaces the previous list.
    source_breakpoints: RwLock<HashMap<String, Vec<CachedBreakpoint>>>,
    /// Function breakpoints (as returned by the last `setFunctionBreakpoints` call).
    function_breakpoints: RwLock<Vec<CachedBreakpoint>>,
}

impl BreakpointCache {
    /// Create an empty breakpoint cache.
    pub fn new() -> Self {
        BreakpointCache {
            source_breakpoints: RwLock::new(HashMap::new()),
            function_breakpoints: RwLock::new(Vec::new()),
        }
    }

    /// Replace the stored breakpoints for a single source file.
    ///
    /// `source_path` must be the resolved absolute path used in the DAP request.
    pub async fn update_source_breakpoints(
        &self,
        source_path: &str,
        request: &[SourceBreakpoint],
        response: &[Breakpoint],
    ) {
        let items: Vec<CachedBreakpoint> = request
            .iter()
            .zip(response.iter())
            .map(|(req, resp)| CachedBreakpoint::from_source(source_path, req, resp))
            .collect();

        let mut map = self.source_breakpoints.write().await;
        map.insert(source_path.to_string(), items);
    }

    /// Replace the stored function breakpoints.
    pub async fn update_function_breakpoints(
        &self,
        request: &[FunctionBreakpoint],
        response: &[Breakpoint],
    ) {
        let items: Vec<CachedBreakpoint> = request
            .iter()
            .zip(response.iter())
            .map(|(req, resp)| CachedBreakpoint::from_function(req, resp))
            .collect();

        *self.function_breakpoints.write().await = items;
    }

    /// Update a single breakpoint by ID, typically from a DAP `breakpoint` event.
    ///
    /// Returns `true` if a matching breakpoint was found and updated.
    pub async fn update_by_id(&self, breakpoint: &Breakpoint) -> bool {
        let Some(id) = breakpoint.id else {
            return false;
        };

        // Search source breakpoints.
        {
            let mut map = self.source_breakpoints.write().await;
            for items in map.values_mut() {
                for item in items.iter_mut() {
                    if item.id == Some(id) {
                        item.verified = breakpoint.verified;
                        item.line = breakpoint.line.or(item.line);
                        item.column = breakpoint.column.or(item.column);
                        item.message.clone_from(&breakpoint.message);
                        if let Some(src) = &breakpoint.source {
                            if let Some(path) = &src.path {
                                item.source_path = Some(path.clone());
                            }
                        }
                        return true;
                    }
                }
            }
        }

        // Search function breakpoints.
        {
            let mut list = self.function_breakpoints.write().await;
            for item in list.iter_mut() {
                if item.id == Some(id) {
                    item.verified = breakpoint.verified;
                    item.line = breakpoint.line.or(item.line);
                    item.column = breakpoint.column.or(item.column);
                    item.message.clone_from(&breakpoint.message);
                    if let Some(src) = &breakpoint.source {
                        if let Some(path) = &src.path {
                            item.source_path = Some(path.clone());
                        }
                    }
                    return true;
                }
            }
        }

        false
    }

    /// Return all cached breakpoints with stored source paths unchanged.
    pub async fn list(&self) -> Vec<CachedBreakpoint> {
        let mut result = Vec::new();

        let map = self.source_breakpoints.read().await;
        for items in map.values() {
            result.extend(items.iter().cloned());
        }
        drop(map);

        result.extend(self.function_breakpoints.read().await.iter().cloned());
        result
    }

    /// Remove all cached breakpoints.
    pub async fn clear(&self) {
        self.source_breakpoints.write().await.clear();
        self.function_breakpoints.write().await.clear();
    }
}

impl Default for BreakpointCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dap_types::types::Source;

    fn src_bp(line: u64, condition: Option<&str>) -> SourceBreakpoint {
        SourceBreakpoint {
            line,
            column: None,
            condition: condition.map(|s| s.to_string()),
            hit_condition: None,
            log_message: None,
            mode: None,
        }
    }

    fn fn_bp(name: &str) -> FunctionBreakpoint {
        FunctionBreakpoint {
            name: name.to_string(),
            condition: None,
            hit_condition: None,
        }
    }

    fn bp(id: Option<u64>, verified: bool, line: Option<u64>) -> Breakpoint {
        Breakpoint {
            id,
            verified,
            message: None,
            source: None,
            line,
            column: None,
            end_line: None,
            end_column: None,
            instruction_reference: None,
            offset: None,
            reason: None,
        }
    }

    #[tokio::test]
    async fn test_source_breakpoints_round_trip() {
        let cache = BreakpointCache::new();
        let request = vec![src_bp(42, Some("x > 5"))];
        let response = vec![bp(Some(1), true, Some(42))];

        cache
            .update_source_breakpoints("/home/user/src/main.cpp", &request, &response)
            .await;

        let list = cache.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, Some(1));
        assert_eq!(list[0].kind, BreakpointKind::Source);
        assert_eq!(list[0].line, Some(42));
        assert_eq!(list[0].condition.as_deref(), Some("x > 5"));
        assert!(list[0].verified);
    }

    #[tokio::test]
    async fn test_source_breakpoints_reverse_mapping() {
        let cache = BreakpointCache::new();
        let request = vec![src_bp(10, None)];
        let response = vec![bp(Some(2), true, Some(10))];

        cache
            .update_source_breakpoints("/home/user/project/src/main.cpp", &request, &response)
            .await;

        let list = cache.list().await;
        assert_eq!(
            list[0].source_path.as_deref(),
            Some("/home/user/project/src/main.cpp")
        );
    }

    #[tokio::test]
    async fn test_source_breakpoints_replace_per_file() {
        let cache = BreakpointCache::new();
        cache
            .update_source_breakpoints("/a/b.cpp", &[src_bp(1, None)], &[bp(None, false, Some(1))])
            .await;
        cache
            .update_source_breakpoints(
                "/a/b.cpp",
                &[src_bp(5, None)],
                &[bp(Some(3), true, Some(5))],
            )
            .await;

        let list = cache.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, Some(3));
        assert_eq!(list[0].line, Some(5));
    }

    #[tokio::test]
    async fn test_function_breakpoints() {
        let cache = BreakpointCache::new();
        let request = vec![fn_bp("main"), fn_bp("foo")];
        let response = vec![bp(Some(10), true, None), bp(Some(11), false, None)];

        cache.update_function_breakpoints(&request, &response).await;

        let list = cache.list().await;
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].function_name.as_deref(), Some("main"));
        assert!(list[0].verified);
        assert_eq!(list[1].function_name.as_deref(), Some("foo"));
        assert!(!list[1].verified);
    }

    #[tokio::test]
    async fn test_update_by_id() {
        let cache = BreakpointCache::new();
        cache
            .update_source_breakpoints("/a.cpp", &[src_bp(1, None)], &[bp(Some(7), false, Some(1))])
            .await;

        let updated = Breakpoint {
            id: Some(7),
            verified: true,
            line: Some(2),
            message: Some("relocated".to_string()),
            source: Some(Source {
                name: Some("a.cpp".to_string()),
                path: Some("/a.cpp".to_string()),
                source_reference: None,
                presentation_hint: None,
                origin: None,
                sources: None,
                adapter_data: None,
                checksums: None,
            }),
            column: None,
            end_line: None,
            end_column: None,
            instruction_reference: None,
            offset: None,
            reason: None,
        };
        assert!(cache.update_by_id(&updated).await);

        let list = cache.list().await;
        assert!(list[0].verified);
        assert_eq!(list[0].line, Some(2));
        assert_eq!(list[0].message.as_deref(), Some("relocated"));
    }

    #[tokio::test]
    async fn test_update_by_id_not_found() {
        let cache = BreakpointCache::new();
        cache
            .update_source_breakpoints("/a.cpp", &[src_bp(1, None)], &[bp(Some(7), false, Some(1))])
            .await;

        let updated = bp(Some(99), true, Some(2));
        assert!(!cache.update_by_id(&updated).await);
    }

    #[tokio::test]
    async fn test_clear() {
        let cache = BreakpointCache::new();
        cache
            .update_source_breakpoints("/a.cpp", &[src_bp(1, None)], &[bp(Some(1), true, Some(1))])
            .await;
        cache
            .update_function_breakpoints(&[fn_bp("main")], &[bp(Some(2), true, None)])
            .await;

        cache.clear().await;
        let list = cache.list().await;
        assert!(list.is_empty());
    }
}
