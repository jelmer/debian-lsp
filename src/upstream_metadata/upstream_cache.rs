//! Cache for guessed upstream metadata values.
//!
//! Uses the `upstream-ontologist` crate to guess field values from the project
//! source tree, and caches results per project root directory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use upstream_ontologist::UpstreamDatum;

/// Thread-safe shared upstream metadata cache.
pub type SharedUpstreamCache = Arc<RwLock<UpstreamCache>>;

/// Cache of guessed upstream metadata values, keyed by project root.
pub struct UpstreamCache {
    /// project_root → (field_name → guessed_values)
    cache: HashMap<PathBuf, HashMap<String, Vec<String>>>,
}

impl UpstreamCache {
    /// Create a new empty cache.
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Check whether guessed values have been cached for a project root.
    pub fn is_cached(&self, project_root: &Path) -> bool {
        self.cache.contains_key(project_root)
    }

    /// Look up cached guessed values for a specific field in a project.
    pub fn get_values(&self, project_root: &Path, field: &str) -> Option<&[String]> {
        self.cache
            .get(project_root)
            .and_then(|fields| fields.get(field))
            .map(|v| v.as_slice())
    }

    /// Run the upstream-ontologist guessers and populate the cache for a project.
    ///
    /// If `net_access` is true, the upstream-ontologist may make HTTP requests
    /// to resolve repository URLs, detect forges, etc.
    pub async fn populate(&mut self, project_root: &Path, net_access: bool) {
        let metadata = match upstream_ontologist::guess_upstream_metadata(
            project_root,
            Some(false),
            Some(net_access),
            None,
            None,
        )
        .await
        {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!("upstream-ontologist error: {e}");
                // Still mark as cached so we don't retry on every keystroke.
                self.cache
                    .insert(project_root.to_path_buf(), HashMap::new());
                return;
            }
        };

        let mut fields: HashMap<String, Vec<String>> = HashMap::new();
        for item in metadata.iter() {
            let field_name = item.datum.field().to_string();
            if let Some(value) = datum_to_string(&item.datum) {
                let entry = fields.entry(field_name).or_default();
                if !entry.contains(&value) {
                    entry.push(value);
                }
            }
        }

        self.cache.insert(project_root.to_path_buf(), fields);
    }
}

/// Extract a string value from an UpstreamDatum for use as a completion.
fn datum_to_string(datum: &UpstreamDatum) -> Option<String> {
    // Most datum types have a direct string representation.
    if let Some(s) = datum.as_str() {
        return Some(s.to_string());
    }

    // For types without as_str, try specific conversions.
    match datum {
        UpstreamDatum::Screenshots(urls) => {
            // Return each URL individually — caller deduplicates.
            urls.first().map(|u| u.to_string())
        }
        UpstreamDatum::Registry(entries) => {
            // Registry is a list of (name, entry) pairs — not a simple scalar.
            // Skip for now; sub-field completions handle these.
            let _ = entries;
            None
        }
        _ => None,
    }
}

/// Create a new shared upstream metadata cache.
pub fn new_shared() -> SharedUpstreamCache {
    Arc::new(RwLock::new(UpstreamCache::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_cache() {
        let cache = UpstreamCache::new();
        assert!(!cache.is_cached(Path::new("/nonexistent")));
        assert_eq!(
            cache.get_values(Path::new("/nonexistent"), "Repository"),
            None
        );
    }

    #[tokio::test]
    async fn test_populate_caches_result() {
        let mut cache = UpstreamCache::new();
        let dir = tempfile::tempdir().unwrap();
        cache.populate(dir.path(), false).await;
        // After populating, the project root should be cached (even if empty).
        assert!(cache.is_cached(dir.path()));
    }
}
