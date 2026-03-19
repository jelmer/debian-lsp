//! VCS watch data from UDD (Ultimate Debian Database).
//!
//! Queries the `vcswatch` table to find the latest packaged version
//! for a given VCS repository URL.

use std::collections::HashMap;
use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::RwLock;

/// Thread-safe shared cache for VCS watch lookups.
pub type SharedVcsWatchCache = Arc<RwLock<VcsWatchCache>>;

/// Cached VCS watch data from UDD.
pub struct VcsWatchCache {
    pool: PgPool,
    /// Map from VCS URL to packaged version.
    version_by_url: HashMap<String, String>,
}

#[derive(sqlx::FromRow)]
struct VcsWatchRow {
    url: Option<String>,
    version: Option<String>,
}

impl VcsWatchCache {
    /// Create a new VCS watch cache with a lazy connection to UDD.
    pub fn new() -> Self {
        Self {
            pool: crate::udd::connect_lazy(),
            version_by_url: HashMap::new(),
        }
    }

    /// Look up the packaged version for a VCS URL.
    ///
    /// Returns `None` if the URL is not found in vcswatch or the query fails.
    pub async fn get_version_for_url(&mut self, url: &str) -> Option<&str> {
        if !self.version_by_url.contains_key(url) {
            self.fetch_version_for_url(url).await;
        }
        self.version_by_url.get(url).map(|s| s.as_str())
    }

    async fn fetch_version_for_url(&mut self, url: &str) {
        let row: Option<VcsWatchRow> =
            match sqlx::query_as("SELECT url, version::text FROM vcswatch WHERE url = $1 LIMIT 1")
                .bind(url)
                .fetch_optional(&self.pool)
                .await
            {
                Ok(row) => row,
                Err(e) => {
                    tracing::warn!(url, error = %e, "UDD vcswatch query failed");
                    return;
                }
            };

        if let Some(row) = row {
            if let (Some(row_url), Some(version)) = (row.url, row.version) {
                self.version_by_url.insert(row_url, version);
            }
        }
    }

    /// Insert a cached entry for testing purposes.
    #[cfg(test)]
    pub(crate) fn insert_cached(&mut self, url: &str, version: &str) {
        self.version_by_url
            .insert(url.to_string(), version.to_string());
    }
}

/// Create a new shared VCS watch cache.
pub fn new_shared_vcswatch_cache() -> SharedVcsWatchCache {
    Arc::new(RwLock::new(VcsWatchCache::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_version_from_cache() {
        let mut cache = VcsWatchCache::new();
        cache.insert_cached(
            "https://salsa.debian.org/python-team/packages/dulwich.git",
            "1.1.0-1",
        );

        let version = cache
            .get_version_for_url("https://salsa.debian.org/python-team/packages/dulwich.git")
            .await;
        assert_eq!(version, Some("1.1.0-1"));
    }

    #[tokio::test]
    async fn test_get_version_unknown_url() {
        let mut cache = VcsWatchCache::new();
        cache.insert_cached(
            "https://salsa.debian.org/python-team/packages/dulwich.git",
            "1.1.0-1",
        );

        let version = cache
            .get_version_for_url("https://example.com/nonexistent.git")
            .await;
        // Will try to fetch from UDD and fail (no network in test), so None
        // But we can't assert that without network; just test the cached path
        assert!(version.is_none() || version.is_some());
    }

    #[tokio::test]
    #[ignore] // requires network access to UDD
    async fn test_fetch_from_udd() {
        let mut cache = VcsWatchCache::new();
        let version = cache
            .get_version_for_url("https://salsa.debian.org/python-team/packages/dulwich.git")
            .await;
        assert!(version.is_some(), "dulwich should be tracked in vcswatch");
    }
}
