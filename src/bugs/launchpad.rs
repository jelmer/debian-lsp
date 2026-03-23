#[cfg(feature = "launchpad")]
use std::collections::BTreeMap;

#[cfg(feature = "launchpad")]
use launchpadlib::r#async::v1_0::{Distribution, DistributionSourcePackage};
#[cfg(feature = "launchpad")]
use launchpadlib::Resource;

use super::BugCache;
#[cfg(feature = "launchpad")]
use super::CachedLaunchpadBugDetails;

/// Launchpad bug data returned to completion providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchpadBugSummary {
    /// Numeric Launchpad bug ID.
    pub id: u32,
    /// Bug title, when available.
    pub title: Option<String>,
    /// Most relevant Launchpad task status, when available.
    pub status: Option<String>,
    /// Whether the package-specific Launchpad task is complete.
    pub done: bool,
}

#[cfg(feature = "launchpad")]
impl BugCache {
    /// Return a single Launchpad bug summary by ID, fetching directly from the
    /// Launchpad API if not already cached.
    pub async fn get_launchpad_bug_summary(&mut self, id: u32) -> Option<LaunchpadBugSummary> {
        if !self.launchpad_bug_details_by_id.contains_key(&id) {
            self.fetch_launchpad_bug_by_id(id).await;
        }
        if self.launchpad_bug_details_by_id.contains_key(&id) {
            Some(self.make_launchpad_summary(id))
        } else {
            None
        }
    }

    /// Fetch a single Launchpad bug by ID and cache it.
    async fn fetch_launchpad_bug_by_id(&mut self, id: u32) {
        let service_root =
            match launchpadlib::r#async::v1_0::service_root(&self.launchpad_client).await {
                Ok(sr) => sr,
                Err(e) => {
                    tracing::warn!(error = %e, "Launchpad service root lookup failed");
                    return;
                }
            };
        let bugs = match service_root.bugs() {
            Some(bugs) => bugs,
            None => {
                tracing::warn!("Launchpad service root missing bugs link");
                return;
            }
        };
        let bug = match bugs.get_by_id(&self.launchpad_client, id).await {
            Ok(bug) => bug,
            Err(e) => {
                tracing::warn!(id, error = %e, "Launchpad single bug lookup failed");
                return;
            }
        };
        let title = if bug.title.trim().is_empty() {
            None
        } else {
            Some(bug.title.clone())
        };
        self.launchpad_bug_details_by_id.insert(
            id,
            CachedLaunchpadBugDetails {
                title,
                // Single-bug fetch doesn't give us task status.
                status: None,
                done: false,
            },
        );
    }

    /// Return Launchpad bug summaries for `package` that match a decimal prefix.
    pub async fn get_launchpad_bug_summaries_with_prefix(
        &mut self,
        package: &str,
        prefix: &str,
    ) -> Vec<LaunchpadBugSummary> {
        self.fetch_launchpad_bugs_for_package(package).await;

        let normalized_prefix = prefix.trim();
        let Some(ids) = self.launchpad_bug_ids_by_package.get(package) else {
            return Vec::new();
        };

        ids.iter()
            .filter(|id| id.to_string().starts_with(normalized_prefix))
            .map(|&id| self.make_launchpad_summary(id))
            .collect()
    }

    /// Build a Launchpad bug summary from cached details for `id`.
    fn make_launchpad_summary(&self, id: u32) -> LaunchpadBugSummary {
        match self.launchpad_bug_details_by_id.get(&id) {
            Some(details) => LaunchpadBugSummary {
                id,
                title: details.title.clone(),
                status: details.status.clone(),
                done: details.done,
            },
            None => LaunchpadBugSummary {
                id,
                title: None,
                status: None,
                done: false,
            },
        }
    }

    /// Fetch Launchpad bug IDs and details for an Ubuntu source package.
    async fn fetch_launchpad_bugs_for_package(&mut self, package: &str) {
        if self.launchpad_bug_ids_by_package.contains_key(package) {
            return;
        }

        let distribution = match self.launchpad_ubuntu_distribution().await {
            Some(distribution) => distribution,
            None => {
                return;
            }
        };

        let source_package_full = match distribution
            .get_source_package(&self.launchpad_client, package)
            .await
        {
            Ok(source_package) => source_package,
            Err(e) => {
                tracing::warn!(package, error = %e, "Launchpad source package lookup failed");
                return;
            }
        };

        let source_package = match source_package_full.self_() {
            Some(source_package) => source_package,
            None => {
                tracing::warn!(
                    package,
                    "Launchpad source package response missing self link"
                );
                return;
            }
        };

        let bug_details_by_id = match self.search_launchpad_tasks(&source_package).await {
            Some(details) => details,
            None => return,
        };

        let mut ids: Vec<u32> = bug_details_by_id.keys().copied().collect();
        ids.sort_unstable();
        for (id, details) in bug_details_by_id {
            self.launchpad_bug_details_by_id.insert(id, details);
        }
        self.launchpad_bug_ids_by_package
            .insert(package.to_string(), ids);
    }

    /// Query Launchpad bug tasks for a source package and index them by bug ID.
    async fn search_launchpad_tasks(
        &self,
        source_package: &DistributionSourcePackage,
    ) -> Option<BTreeMap<u32, CachedLaunchpadBugDetails>> {
        let mut tasks = match source_package
            .search_tasks(
                &self.launchpad_client,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .await
        {
            Ok(tasks) => tasks,
            Err(e) => {
                tracing::warn!(error = %e, "Launchpad bug query failed");
                return None;
            }
        };

        let mut by_id: BTreeMap<u32, CachedLaunchpadBugDetails> = BTreeMap::new();
        let source_package_url = source_package.url().clone();
        let mut index = 0usize;
        loop {
            let task = match tasks.get(index).await {
                Ok(task) => task,
                Err(e) => {
                    tracing::warn!(error = %e, "Launchpad bug page iteration failed");
                    return None;
                }
            };

            let Some(task) = task else {
                break;
            };
            index += 1;

            // Keep only the task for the source package we're querying.
            if task.target_link != source_package_url {
                continue;
            }

            let bug = match task.bug().get(&self.launchpad_client).await {
                Ok(bug) => bug,
                Err(e) => {
                    tracing::warn!(error = %e, "Launchpad bug lookup failed");
                    continue;
                }
            };

            let Some(id) = u32::try_from(bug.id).ok() else {
                continue;
            };

            let title = if bug.title.trim().is_empty() {
                None
            } else {
                Some(bug.title.clone())
            };
            let status_value = task.status.to_string();
            let status = if status_value.trim().is_empty() {
                None
            } else {
                Some(status_value)
            };
            let done = task.is_complete;

            match by_id.get_mut(&id) {
                Some(existing) => {
                    existing.done = done;
                    if existing.title.is_none() && title.is_some() {
                        existing.title = title.clone();
                    }
                    if existing.status.is_none() && status.is_some() {
                        existing.status = status.clone();
                    }
                }
                None => {
                    by_id.insert(
                        id,
                        CachedLaunchpadBugDetails {
                            title,
                            status,
                            done,
                        },
                    );
                }
            }
        }

        Some(by_id)
    }

    /// Resolve the Launchpad `ubuntu` distribution resource.
    async fn launchpad_ubuntu_distribution(&self) -> Option<Distribution> {
        let service_root =
            match launchpadlib::r#async::v1_0::service_root(&self.launchpad_client).await {
                Ok(service_root) => service_root,
                Err(e) => {
                    tracing::warn!(error = %e, "Launchpad service root lookup failed");
                    return None;
                }
            };

        let distributions = match service_root.distributions() {
            Some(distributions) => distributions,
            None => {
                tracing::warn!("Launchpad service root missing distributions link");
                return None;
            }
        };

        let distribution_full = match distributions
            .get_by_name(&self.launchpad_client, "ubuntu")
            .await
        {
            Ok(distribution) => distribution,
            Err(e) => {
                tracing::warn!(error = %e, "Launchpad ubuntu distribution lookup failed");
                return None;
            }
        };

        let distribution = match distribution_full.self_() {
            Some(distribution) => distribution,
            None => {
                tracing::warn!("Launchpad ubuntu distribution response missing self link");
                return None;
            }
        };

        Some(distribution)
    }

    /// Pre-fetch Launchpad bug IDs and their details for an Ubuntu source package.
    pub async fn prefetch_launchpad_bugs_for_package(&mut self, package: &str) {
        self.fetch_launchpad_bugs_for_package(package).await;
    }

    #[cfg(test)]
    pub(crate) fn insert_cached_launchpad_bugs_for_package(
        &mut self,
        package: &str,
        bugs: Vec<(u32, Option<&str>, Option<&str>, bool)>,
    ) {
        let mut sorted_unique_ids = std::collections::BTreeSet::new();

        for (id, title, status, done) in bugs {
            sorted_unique_ids.insert(id);
            self.launchpad_bug_details_by_id.insert(
                id,
                CachedLaunchpadBugDetails {
                    title: title.map(ToString::to_string),
                    status: status.map(ToString::to_string),
                    done,
                },
            );
        }

        self.launchpad_bug_ids_by_package
            .insert(package.to_string(), sorted_unique_ids.into_iter().collect());
    }
}

#[cfg(not(feature = "launchpad"))]
impl BugCache {
    /// Return a single Launchpad bug summary by ID.
    pub async fn get_launchpad_bug_summary(&mut self, _id: u32) -> Option<LaunchpadBugSummary> {
        None
    }

    /// Return Launchpad bug summaries for `package` that match a decimal prefix.
    pub async fn get_launchpad_bug_summaries_with_prefix(
        &mut self,
        _package: &str,
        _prefix: &str,
    ) -> Vec<LaunchpadBugSummary> {
        Vec::new()
    }

    /// Pre-fetch Launchpad bug IDs and their details for an Ubuntu source package.
    pub async fn prefetch_launchpad_bugs_for_package(&mut self, _package: &str) {}

    #[cfg(test)]
    pub(crate) fn insert_cached_launchpad_bugs_for_package(
        &mut self,
        _package: &str,
        _bugs: Vec<(u32, Option<&str>, Option<&str>, bool)>,
    ) {
    }
}

#[cfg(all(test, feature = "launchpad"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_launchpad_bug_summaries_with_prefix_from_cache() {
        let mut cache = BugCache::new(crate::udd::shared_pool());
        cache.insert_cached_launchpad_bugs_for_package(
            "foo",
            vec![
                (123456, Some("Launchpad crash report"), Some("New"), false),
                (123499, None, Some("Fix Released"), true),
                (888888, Some("Unrelated issue"), Some("Confirmed"), false),
            ],
        );

        let summaries = cache
            .get_launchpad_bug_summaries_with_prefix("foo", "1234")
            .await;
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].id, 123456);
        assert_eq!(
            summaries[0].title.as_deref(),
            Some("Launchpad crash report")
        );
        assert_eq!(summaries[0].status.as_deref(), Some("New"));
        assert!(!summaries[0].done);
        assert_eq!(summaries[1].id, 123499);
        assert_eq!(summaries[1].title, None);
        assert_eq!(summaries[1].status.as_deref(), Some("Fix Released"));
        assert!(summaries[1].done);
    }

    #[tokio::test]
    async fn test_launchpad_bug_summary_done_is_package_specific() {
        let mut cache = BugCache::new(crate::udd::shared_pool());
        cache.insert_cached_launchpad_bugs_for_package(
            "foo",
            vec![(123456, Some("Launchpad crash report"), Some("New"), false)],
        );

        let summaries = cache
            .get_launchpad_bug_summaries_with_prefix("foo", "")
            .await;
        assert_eq!(summaries.len(), 1);
        assert!(!summaries[0].done);
    }
}
