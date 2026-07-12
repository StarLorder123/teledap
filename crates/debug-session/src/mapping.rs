//! Path mapping — bidirectional translation between AI-relative paths and
//! system absolute paths.
//!
//! AI assistants work with relative paths like `src/main.cpp`, but codelldb
//! and DAP operate on absolute paths like `/home/user/project/src/main.cpp`.
//! This module allows registering path aliases and resolving in both directions.
//!
//! # Usage
//!
//! ```ignore
//! let mut mapper = PathMapper::new();
//! mapper.register_base_dir("/home/user/project");
//! mapper.register_alias("src", "/home/user/project/src");
//!
//! // AI → system
//! let abs = mapper.resolve("src/main.cpp");
//! // → Some("/home/user/project/src/main.cpp")
//!
//! // System → AI
//! let rel = mapper.reverse("/home/user/project/src/main.cpp");
//! // → Some("src/main.cpp")
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Bi-directional path mapper for AI ↔ system path translation.
#[derive(Debug, Clone, Default)]
pub struct PathMapper {
    /// Registered aliases: alias → resolved absolute path.
    aliases: HashMap<String, PathBuf>,
    /// Reverse lookup cache: absolute path → alias prefix.
    reverse_cache: HashMap<PathBuf, String>,
    /// Base directories for relative path resolution.
    base_dirs: Vec<PathBuf>,
}

impl PathMapper {
    /// Create an empty path mapper.
    pub fn new() -> Self {
        PathMapper {
            aliases: HashMap::new(),
            reverse_cache: HashMap::new(),
            base_dirs: Vec::new(),
        }
    }

    /// Register a base directory. When resolving a relative path that doesn't
    /// match any alias prefix, the mapper will try joining with each base dir.
    pub fn register_base_dir(&mut self, dir: impl AsRef<Path>) {
        self.base_dirs.push(dir.as_ref().to_path_buf());
    }

    /// Register an alias → absolute path mapping.
    ///
    /// After registration, any path starting with `alias` will resolve to
    /// `absolute_path` (with the alias prefix replaced).
    ///
    /// # Example
    ///
    /// ```
    /// use debug_session::PathMapper;
    /// let mut m = PathMapper::new();
    /// m.register_alias("src", "/home/user/project/src");
    /// assert_eq!(m.resolve("src/main.cpp"), Some("/home/user/project/src/main.cpp".into()));
    /// ```
    pub fn register_alias(&mut self, alias: impl Into<String>, absolute_path: impl AsRef<Path>) {
        let alias = alias.into();
        let abs = absolute_path.as_ref().to_path_buf();
        self.reverse_cache.insert(abs.clone(), alias.clone());
        self.aliases.insert(alias, abs);
    }

    /// Register multiple alias mappings at once.
    pub fn register_aliases(
        &mut self,
        mappings: impl IntoIterator<Item = (impl Into<String>, impl AsRef<Path>)>,
    ) {
        for (alias, path) in mappings {
            self.register_alias(alias, path);
        }
    }

    /// Resolve a potentially relative or aliased path to an absolute system path.
    ///
    /// Resolution order:
    /// 1. If the path is already absolute, return it as-is.
    /// 2. If the path starts with a registered alias, replace the prefix.
    /// 3. If base directories are registered, try joining with each.
    /// 4. Otherwise, return `None`.
    pub fn resolve(&self, path: &str) -> Option<String> {
        let p = Path::new(path);

        // Already absolute
        if p.is_absolute() {
            return Some(normalize_path(path));
        }

        // Try alias prefix matching (longest match first)
        let mut sorted_aliases: Vec<(&String, &PathBuf)> = self.aliases.iter().collect();
        sorted_aliases.sort_by_key(|(a, _)| -(a.len() as i64)); // longest first

        for (alias, abs_dir) in &sorted_aliases {
            // Normalize separators for comparison
            let normalized_path = path.replace('\\', "/");
            let normalized_alias = alias.replace('\\', "/");

            if normalized_path == normalized_alias {
                return Some(abs_dir.to_string_lossy().to_string());
            }

            let prefix = if normalized_alias.ends_with('/') {
                normalized_alias.clone()
            } else {
                format!("{normalized_alias}/")
            };

            if normalized_path.starts_with(&prefix) {
                let remainder = &normalized_path[prefix.len()..];
                let resolved = abs_dir.join(remainder);
                return Some(normalize_path(&resolved.to_string_lossy()));
            }
        }

        // Try base directories
        for base in &self.base_dirs {
            let candidate = base.join(path);
            if candidate.exists() {
                return Some(normalize_path(&candidate.to_string_lossy()));
            }
        }

        // Fallback: try the first base directory even if the file doesn't exist
        // (it may not exist yet, e.g., when setting a breakpoint before build)
        if let Some(base) = self.base_dirs.first() {
            let candidate = base.join(path);
            return Some(normalize_path(&candidate.to_string_lossy()));
        }

        None
    }

    /// Reverse-resolve an absolute system path to the most specific registered alias.
    ///
    /// Returns the aliased path, or `None` if no alias matches.
    pub fn reverse(&self, absolute_path: &str) -> Option<String> {
        let abs = Path::new(absolute_path);

        // Find the longest matching alias prefix (most specific)
        let mut best: Option<(&str, &Path)> = None;
        for (alias, abs_dir) in &self.aliases {
            if abs.starts_with(abs_dir) {
                match best {
                    None => best = Some((alias, abs_dir)),
                    Some((_, existing)) => {
                        // Longer matched prefix = more specific
                        if abs_dir.as_os_str().len() > existing.as_os_str().len() {
                            best = Some((alias, abs_dir));
                        }
                    }
                }
            }
        }

        if let Some((alias, abs_dir)) = best {
            let remainder = abs.strip_prefix(abs_dir).ok()?;
            if remainder.as_os_str().is_empty() {
                return Some(alias.to_string());
            }
            let normalized_alias = alias.to_string().replace('\\', "/");
            let normalized_rem = remainder.to_string_lossy().replace('\\', "/");
            if normalized_rem.starts_with('/') {
                Some(format!("{normalized_alias}{normalized_rem}"))
            } else {
                Some(format!("{normalized_alias}/{normalized_rem}"))
            }
        } else {
            // Try stripping base directories
            for base in &self.base_dirs {
                if let Ok(rem) = abs.strip_prefix(base) {
                    return Some(rem.to_string_lossy().to_string());
                }
            }
            None
        }
    }

    /// Returns the number of registered aliases.
    pub fn alias_count(&self) -> usize {
        self.aliases.len()
    }

    /// Returns the number of registered base directories.
    pub fn base_dir_count(&self) -> usize {
        self.base_dirs.len()
    }

    /// Returns true if no aliases or base directories are registered.
    pub fn is_empty(&self) -> bool {
        self.aliases.is_empty() && self.base_dirs.is_empty()
    }

    /// Remove all registered aliases and base directories.
    pub fn clear(&mut self) {
        self.aliases.clear();
        self.reverse_cache.clear();
        self.base_dirs.clear();
    }
}

/// Normalize path separators for the current platform.
fn normalize_path(path: &str) -> String {
    Path::new(path)
        .to_string_lossy()
        .to_string()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_absolute_unchanged() {
        let mapper = PathMapper::new();
        // Use a platform-appropriate absolute path
        let abs_path = if cfg!(windows) {
            "C:/Users/test/main.cpp"
        } else {
            "/home/user/main.cpp"
        };
        let result = mapper.resolve(abs_path);
        assert_eq!(result, Some(abs_path.into()));
    }

    #[test]
    fn test_resolve_alias_exact_match() {
        let mut mapper = PathMapper::new();
        mapper.register_alias("src", "/home/user/project/src");
        let result = mapper.resolve("src");
        assert_eq!(result, Some("/home/user/project/src".into()));
    }

    #[test]
    fn test_resolve_alias_with_subpath() {
        let mut mapper = PathMapper::new();
        mapper.register_alias("src", "/home/user/project/src");
        let result = mapper.resolve("src/main.cpp");
        assert_eq!(result, Some("/home/user/project/src/main.cpp".into()));
    }

    #[test]
    fn test_resolve_longest_alias_wins() {
        let mut mapper = PathMapper::new();
        mapper.register_alias("src", "/home/user/project/src");
        mapper.register_alias("src/sub", "/home/user/project/src/subdir");
        // "src/sub/main.cpp" should match "src/sub" (longer), not "src"
        let result = mapper.resolve("src/sub/main.cpp");
        assert_eq!(
            result,
            Some("/home/user/project/src/subdir/main.cpp".into())
        );
    }

    #[test]
    fn test_resolve_base_dir_fallback() {
        let mut mapper = PathMapper::new();
        mapper.register_base_dir("/home/user/project");
        // File doesn't exist, but fallback to first base dir
        let result = mapper.resolve("lib/helper.cpp");
        assert_eq!(result, Some("/home/user/project/lib/helper.cpp".into()));
    }

    #[test]
    fn test_resolve_none_without_registration() {
        let mapper = PathMapper::new();
        let result = mapper.resolve("src/main.cpp");
        assert_eq!(result, None);
    }

    #[test]
    fn test_reverse_exact() {
        let mut mapper = PathMapper::new();
        mapper.register_alias("src", "/home/user/project/src");
        let result = mapper.reverse("/home/user/project/src/main.cpp");
        assert_eq!(result, Some("src/main.cpp".into()));
    }

    #[test]
    fn test_reverse_most_specific() {
        let mut mapper = PathMapper::new();
        mapper.register_alias("proj", "/home/user/project");
        mapper.register_alias("src", "/home/user/project/src");
        // Should match "src" (more specific), not "proj"
        let result = mapper.reverse("/home/user/project/src/main.cpp");
        assert_eq!(result, Some("src/main.cpp".into()));
    }

    #[test]
    fn test_reverse_base_dir() {
        let mut mapper = PathMapper::new();
        mapper.register_base_dir("/home/user/project");
        let result = mapper.reverse("/home/user/project/src/main.cpp");
        assert_eq!(result, Some("src/main.cpp".into()));
    }

    #[test]
    fn test_reverse_no_match() {
        let mapper = PathMapper::new();
        let result = mapper.reverse("/home/user/project/src/main.cpp");
        assert_eq!(result, None);
    }

    #[test]
    fn test_reverse_alias_exact_directory() {
        let mut mapper = PathMapper::new();
        mapper.register_alias("src", "/home/user/project/src");
        let result = mapper.reverse("/home/user/project/src");
        assert_eq!(result, Some("src".into()));
    }

    #[test]
    fn test_clear() {
        let mut mapper = PathMapper::new();
        mapper.register_alias("src", "/home/user/project/src");
        mapper.register_base_dir("/home/user");
        assert!(!mapper.is_empty());
        mapper.clear();
        assert!(mapper.is_empty());
        assert_eq!(mapper.alias_count(), 0);
        assert_eq!(mapper.base_dir_count(), 0);
    }

    #[test]
    fn test_windows_separators() {
        let mut mapper = PathMapper::new();
        mapper.register_alias("src", "C:/project/src");
        // Input with backslashes should still match
        let result = mapper.resolve("src\\main.cpp");
        assert_eq!(result, Some("C:/project/src/main.cpp".into()));
    }
}
