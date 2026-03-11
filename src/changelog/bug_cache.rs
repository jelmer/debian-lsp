use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use tokio::sync::RwLock;

/// Shared bug cache used by changelog completion.
pub type SharedBugCache = Arc<RwLock<DebbugsCache>>;

/// Cache for open Debian bug IDs keyed by source package name.
#[derive(Default)]
pub struct DebbugsCache {
    client: debbugs::Debbugs,
    open_bug_ids_by_package: HashMap<String, Vec<u32>>,
}

impl DebbugsCache {
    /// Create a new empty bug cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return open Debian bug IDs for `package`, loading and caching from Debbugs if needed.
    pub async fn get_open_bug_ids_for_package(&mut self, package: &str) -> Vec<u32> {
        if let Some(cached) = self.open_bug_ids_by_package.get(package) {
            return cached.clone();
        }

        let query = debbugs::SearchQuery {
            package: Some(package),
            status: Some(debbugs::BugStatus::Open),
            ..Default::default()
        };

        match self.client.get_bugs(&query).await {
            Ok(ids) => {
                let sorted_unique: Vec<u32> = ids
                    .into_iter()
                    .filter_map(|id| u32::try_from(id).ok())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                self.open_bug_ids_by_package
                    .insert(package.to_string(), sorted_unique.clone());
                sorted_unique
            }
            Err(_) => Vec::new(),
        }
    }

    /// Return open Debian bug IDs for `package` that match a decimal prefix.
    pub async fn get_open_bug_ids_with_prefix(&mut self, package: &str, prefix: &str) -> Vec<u32> {
        let normalized_prefix = prefix.trim();
        self.get_open_bug_ids_for_package(package)
            .await
            .into_iter()
            .filter(|id| id.to_string().starts_with(normalized_prefix))
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn insert_cached_open_bug_ids_for_package(
        &mut self,
        package: &str,
        bug_ids: Vec<u32>,
    ) {
        let sorted_unique: Vec<u32> = bug_ids
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        self.open_bug_ids_by_package
            .insert(package.to_string(), sorted_unique);
    }
}

/// Create a new shared cache for Debbugs bug IDs.
pub fn new_shared_bug_cache() -> SharedBugCache {
    Arc::new(RwLock::new(DebbugsCache::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_open_bug_ids_with_prefix_from_cache() {
        let mut cache = DebbugsCache::new();
        cache.insert_cached_open_bug_ids_for_package("foo", vec![123456, 123499, 888888]);

        let ids = tokio_test::block_on(cache.get_open_bug_ids_with_prefix("foo", "1234"));
        assert_eq!(ids, vec![123456, 123499]);
    }
}
