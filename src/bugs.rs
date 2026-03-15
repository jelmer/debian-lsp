use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use launchpadlib::r#async::v1_0::{Distribution, DistributionSourcePackage};
use tokio::sync::RwLock;

/// Thread-safe shared cache for Debian bug tracker lookups.
pub type SharedBugCache = Arc<RwLock<BugCache>>;

const LAUNCHPAD_CONSUMER_KEY: &str = "debian-lsp";

/// Cached bug data used by changelog completions.
pub struct BugCache {
    pool: crate::udd::SharedPool,
    bug_ids_by_package: HashMap<String, Vec<u32>>,
    bug_details_by_id: HashMap<u32, CachedBugDetails>,
    launchpad_client: launchpadlib::r#async::Client,
    launchpad_bug_ids_by_package: HashMap<String, Vec<u32>>,
    launchpad_bug_details_by_id: HashMap<u32, CachedLaunchpadBugDetails>,
}

/// Cached details for a single bug report.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CachedBugDetails {
    title: Option<String>,
    severity: Option<String>,
    done: bool,
    tags: Option<String>,
    forwarded: Option<String>,
    originator: Option<String>,
}

/// Cached details for a Launchpad bug relevant to completion.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CachedLaunchpadBugDetails {
    title: Option<String>,
    status: Option<String>,
    done: bool,
}

/// Bug data returned to completion providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BugSummary {
    /// Numeric Debian bug ID.
    pub id: u32,
    /// Bug title, when available.
    pub title: Option<String>,
    /// Bug severity (e.g. "serious", "normal", "wishlist").
    pub severity: Option<String>,
    /// Whether the bug has been marked as done/resolved.
    pub done: bool,
    /// Tags associated with the bug (e.g. "patch", "confirmed").
    pub tags: Option<String>,
    /// Where the bug has been forwarded to, if anywhere.
    pub forwarded: Option<String>,
    /// Email address of the person who reported the bug.
    pub originator: Option<String>,
}

/// Launchpad bug data returned to completion providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchpadBugSummary {
    /// Numeric Launchpad bug ID.
    pub id: u32,
    /// Bug title, when available.
    pub title: Option<String>,
    /// Most relevant Launchpad task status, when available.
    pub status: Option<String>,
    /// Whether the bug is complete across known tasks.
    pub done: bool,
}

#[derive(sqlx::FromRow)]
struct BugRow {
    id: i32,
    title: Option<String>,
    severity: Option<String>,
    done: Option<String>,
    tags: Option<String>,
    forwarded: Option<String>,
    submitter: Option<String>,
}

impl BugCache {
    /// Create a new bug cache using the given UDD connection pool.
    pub fn new(pool: crate::udd::SharedPool) -> Self {
        Self {
            pool,
            bug_ids_by_package: HashMap::new(),
            bug_details_by_id: HashMap::new(),
            launchpad_client: launchpadlib::r#async::Client::anonymous(LAUNCHPAD_CONSUMER_KEY),
            launchpad_bug_ids_by_package: HashMap::new(),
            launchpad_bug_details_by_id: HashMap::new(),
        }
    }

    /// Fetch bug IDs and details for a package from UDD in a single query.
    async fn fetch_bugs_for_package(&mut self, package: &str) {
        if self.bug_ids_by_package.contains_key(package) {
            return;
        }

        let rows: Vec<BugRow> = match sqlx::query_as(
            "SELECT b.id, b.title, b.severity::text, b.done, b.forwarded, b.submitter, \
                    (SELECT string_agg(t.tag, ', ') FROM bugs_tags t WHERE t.id = b.id) AS tags \
             FROM bugs b \
             WHERE b.source = $1 \
             ORDER BY b.id",
        )
        .bind(package)
        .fetch_all(&*self.pool)
        .await
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!(package, error = %e, "UDD bug query failed");
                return;
            }
        };

        let mut ids = Vec::new();
        for row in rows {
            let Some(id) = u32::try_from(row.id).ok() else {
                continue;
            };
            ids.push(id);
            self.bug_details_by_id.insert(
                id,
                CachedBugDetails {
                    title: row.title,
                    severity: row.severity,
                    done: row.done.as_ref().is_some_and(|d| !d.is_empty()),
                    tags: row.tags,
                    forwarded: row.forwarded,
                    originator: row.submitter,
                },
            );
        }

        self.bug_ids_by_package.insert(package.to_string(), ids);
    }

    /// Return Debian bug summaries for `package` that match a decimal prefix.
    pub async fn get_bug_summaries_with_prefix(
        &mut self,
        package: &str,
        prefix: &str,
    ) -> Vec<BugSummary> {
        self.fetch_bugs_for_package(package).await;

        let normalized_prefix = prefix.trim();
        let Some(ids) = self.bug_ids_by_package.get(package) else {
            return Vec::new();
        };

        ids.iter()
            .filter(|id| id.to_string().starts_with(normalized_prefix))
            .map(|&id| self.make_summary(id))
            .collect()
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

    fn make_summary(&self, id: u32) -> BugSummary {
        match self.bug_details_by_id.get(&id) {
            Some(details) => BugSummary {
                id,
                title: details.title.clone(),
                severity: details.severity.clone(),
                done: details.done,
                tags: details.tags.clone(),
                forwarded: details.forwarded.clone(),
                originator: details.originator.clone(),
            },
            None => BugSummary {
                id,
                title: None,
                severity: None,
                done: false,
                tags: None,
                forwarded: None,
                originator: None,
            },
        }
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

    /// Query Launchpad bug tasks for a source package and fold them by bug ID.
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

            let Some(id) = Self::launchpad_bug_id_from_link(task.bug_link.as_str()) else {
                continue;
            };

            let title = if task.title.trim().is_empty() {
                None
            } else {
                Some(task.title.clone())
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
                    let was_done = existing.done;
                    existing.done &= done;
                    if existing.title.is_none() && title.is_some() {
                        existing.title = title;
                    }
                    if !done && was_done {
                        existing.status = status.clone();
                    } else if existing.status.is_none() && status.is_some() {
                        existing.status = status;
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

    /// Extract the numeric bug ID from a Launchpad bug API URL.
    fn launchpad_bug_id_from_link(link: &str) -> Option<u32> {
        let trimmed = link.trim_end_matches('/');
        trimmed.rsplit('/').next()?.parse().ok()
    }

    /// Pre-fetch open bug IDs and their details for a package.
    ///
    /// Call this in the background so the data is cached before the user
    /// triggers completion.
    pub async fn prefetch_bugs_for_package(&mut self, package: &str) {
        self.fetch_bugs_for_package(package).await;
    }

    /// Pre-fetch Launchpad bug IDs and their details for an Ubuntu source package.
    pub async fn prefetch_launchpad_bugs_for_package(&mut self, package: &str) {
        self.fetch_launchpad_bugs_for_package(package).await;
    }

    #[cfg(test)]
    pub(crate) fn insert_cached_open_bugs_for_package(
        &mut self,
        package: &str,
        bugs: Vec<(u32, Option<&str>)>,
    ) {
        let mut sorted_unique_ids = std::collections::BTreeSet::new();

        for (id, title) in bugs {
            sorted_unique_ids.insert(id);
            self.bug_details_by_id.insert(
                id,
                CachedBugDetails {
                    title: title.map(ToString::to_string),
                    severity: None,
                    done: false,
                    tags: None,
                    forwarded: None,
                    originator: None,
                },
            );
        }

        self.bug_ids_by_package
            .insert(package.to_string(), sorted_unique_ids.into_iter().collect());
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

/// Create a new shared cache for bug data from UDD.
pub fn new_shared_bug_cache(pool: crate::udd::SharedPool) -> SharedBugCache {
    Arc::new(RwLock::new(BugCache::new(pool)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_bug_summaries_with_prefix_from_cache() {
        let mut cache = BugCache::new(crate::udd::shared_pool());
        cache.insert_cached_open_bugs_for_package(
            "foo",
            vec![
                (123456, Some("Fix crash on startup")),
                (123499, None),
                (888888, Some("Unrelated issue")),
            ],
        );

        let summaries = cache.get_bug_summaries_with_prefix("foo", "1234").await;
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].id, 123456);
        assert_eq!(summaries[0].title.as_deref(), Some("Fix crash on startup"));
        assert_eq!(summaries[1].id, 123499);
        assert_eq!(summaries[1].title, None);
    }

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

    #[test]
    fn test_launchpad_bug_id_from_link() {
        assert_eq!(
            BugCache::launchpad_bug_id_from_link("https://api.launchpad.net/1.0/bugs/123456"),
            Some(123456)
        );
        assert_eq!(
            BugCache::launchpad_bug_id_from_link("https://api.launchpad.net/1.0/bugs/123456/"),
            Some(123456)
        );
        assert_eq!(
            BugCache::launchpad_bug_id_from_link("https://api.launchpad.net/1.0/bugs/not-a-number"),
            None
        );
    }

    #[tokio::test]
    #[ignore] // requires network access to UDD
    async fn test_fetch_bugs_from_udd() {
        let mut cache = BugCache::new(crate::udd::shared_pool());
        let summaries = cache.get_bug_summaries_with_prefix("lintian", "").await;
        assert!(!summaries.is_empty(), "lintian should have bugs in UDD");
        // Every summary should have a title
        assert!(
            summaries.iter().any(|s| s.title.is_some()),
            "at least some bugs should have titles"
        );
    }
}
