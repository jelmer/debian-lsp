use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use tokio::sync::RwLock;

/// Thread-safe shared cache for Debian bug tracker lookups.
pub type SharedBugCache = Arc<RwLock<DebbugsCache>>;

/// Cached bug data used by changelog completions.
#[derive(Default)]
pub struct DebbugsCache {
    client: debbugs::Debbugs,
    open_bug_ids_by_package: HashMap<String, Vec<u32>>,
    bug_titles_by_id: HashMap<u32, Option<String>>,
}

/// Minimal bug data returned to completion providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BugSummary {
    /// Numeric Debian bug ID.
    pub id: u32,
    /// Bug title from Debbugs, when available.
    pub title: Option<String>,
}

impl DebbugsCache {
    /// Create a new empty bug cache.
    pub fn new() -> Self {
        Self::default()
    }

    async fn get_open_bug_ids_for_package(&mut self, package: &str) -> Vec<u32> {
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

    /// Return open Debian bug summaries for `package` that match a decimal prefix.
    pub async fn get_open_bug_summaries_with_prefix(
        &mut self,
        package: &str,
        prefix: &str,
    ) -> Vec<BugSummary> {
        let normalized_prefix = prefix.trim();
        let matching_ids: Vec<u32> = self
            .get_open_bug_ids_for_package(package)
            .await
            .into_iter()
            .filter(|id| id.to_string().starts_with(normalized_prefix))
            .collect();

        self.cache_bug_titles(&matching_ids).await;

        matching_ids
            .into_iter()
            .map(|id| BugSummary {
                id,
                title: self.bug_titles_by_id.get(&id).cloned().flatten(),
            })
            .collect()
    }

    async fn cache_bug_titles(&mut self, bug_ids: &[u32]) {
        let ids_to_load: Vec<(u32, debbugs::BugId)> = bug_ids
            .iter()
            .copied()
            .filter(|id| !self.bug_titles_by_id.contains_key(id))
            .filter_map(|id| i32::try_from(id).ok().map(|debbugs_id| (id, debbugs_id)))
            .collect();

        if ids_to_load.is_empty() {
            return;
        }

        let debbugs_ids: Vec<debbugs::BugId> = ids_to_load
            .iter()
            .map(|(_, debbugs_id)| *debbugs_id)
            .collect();

        if let Ok(reports) = self.client.get_status(&debbugs_ids).await {
            for (id, debbugs_id) in ids_to_load {
                let title = reports
                    .get(&debbugs_id)
                    .and_then(|report| report.subject.as_deref())
                    .map(str::trim)
                    .filter(|title| !title.is_empty())
                    .map(ToString::to_string);
                self.bug_titles_by_id.insert(id, title);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn insert_cached_open_bugs_for_package(
        &mut self,
        package: &str,
        bugs: Vec<(u32, Option<&str>)>,
    ) {
        let mut sorted_unique_ids = BTreeSet::new();

        for (id, title) in bugs {
            sorted_unique_ids.insert(id);
            self.bug_titles_by_id
                .insert(id, title.map(ToString::to_string));
        }

        self.open_bug_ids_by_package
            .insert(package.to_string(), sorted_unique_ids.into_iter().collect());
    }
}

/// Create a new shared cache for Debbugs bug data.
pub fn new_shared_bug_cache() -> SharedBugCache {
    Arc::new(RwLock::new(DebbugsCache::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_open_bug_summaries_with_prefix_from_cache() {
        let mut cache = DebbugsCache::new();
        cache.insert_cached_open_bugs_for_package(
            "foo",
            vec![
                (123456, Some("Fix crash on startup")),
                (123499, None),
                (888888, Some("Unrelated issue")),
            ],
        );

        let summaries =
            tokio_test::block_on(cache.get_open_bug_summaries_with_prefix("foo", "1234"));
        assert_eq!(
            summaries,
            vec![
                BugSummary {
                    id: 123456,
                    title: Some("Fix crash on startup".to_string()),
                },
                BugSummary {
                    id: 123499,
                    title: None,
                },
            ]
        );
    }
}
