//! VCS watch data from UDD (Ultimate Debian Database).
//!
//! Queries the `vcswatch` table to find the latest packaged version
//! for a given VCS repository URL.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

/// Thread-safe shared cache for VCS watch lookups.
pub type SharedVcsWatchCache = Arc<RwLock<VcsWatchCache>>;

/// Cached VCS watch data from UDD.
pub struct VcsWatchCache {
    pool: crate::udd::SharedPool,
    /// Map from VCS URL to packaged version. `None` means "looked up, not found".
    version_by_url: HashMap<String, Option<String>>,
}

#[derive(sqlx::FromRow)]
struct VcsWatchRow {
    url: Option<String>,
    version: Option<String>,
}

impl VcsWatchCache {
    /// Create a new VCS watch cache using the given UDD connection pool.
    pub fn new(pool: crate::udd::SharedPool) -> Self {
        Self {
            pool,
            version_by_url: HashMap::new(),
        }
    }

    /// Look up the packaged version for a VCS URL, fetching if needed.
    ///
    /// Returns `None` if the URL is not found in vcswatch or the query fails.
    pub async fn get_version_for_url(&mut self, url: &str) -> Option<&str> {
        if !self.version_by_url.contains_key(url) {
            self.fetch_version_for_url(url).await;
        }
        self.get_cached_version_for_url(url)
    }

    /// Look up the packaged version from cache only, without fetching.
    pub fn get_cached_version_for_url(&self, url: &str) -> Option<&str> {
        self.version_by_url.get(url).and_then(|v| v.as_deref())
    }

    /// Returns `true` if this URL has been looked up (hit or miss).
    pub fn is_cached(&self, url: &str) -> bool {
        self.version_by_url.contains_key(url)
    }

    async fn fetch_version_for_url(&mut self, url: &str) {
        let row: Option<VcsWatchRow> =
            match sqlx::query_as("SELECT url, version::text FROM vcswatch WHERE url = $1 LIMIT 1")
                .bind(url)
                .fetch_optional(&*self.pool)
                .await
            {
                Ok(row) => row,
                Err(e) => {
                    tracing::warn!(url, error = %e, "UDD vcswatch query failed");
                    return;
                }
            };

        match row {
            Some(VcsWatchRow {
                url: Some(row_url),
                version,
            }) => {
                self.version_by_url.insert(row_url, version);
            }
            _ => {
                // Not found — cache as None to avoid re-querying
                self.version_by_url.insert(url.to_string(), None);
            }
        }
    }

    /// Insert a cached entry for testing purposes.
    #[cfg(test)]
    pub(crate) fn insert_cached(&mut self, url: &str, version: &str) {
        self.version_by_url
            .insert(url.to_string(), Some(version.to_string()));
    }
}

/// Create a new shared VCS watch cache.
pub fn new_shared_vcswatch_cache(pool: crate::udd::SharedPool) -> SharedVcsWatchCache {
    Arc::new(RwLock::new(VcsWatchCache::new(pool)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_version_from_cache() {
        let mut cache = VcsWatchCache::new(crate::udd::shared_pool());
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
        let mut cache = VcsWatchCache::new(crate::udd::shared_pool());
        cache.insert_cached(
            "https://salsa.debian.org/python-team/packages/dulwich.git",
            "1.1.0-1",
        );

        // This URL is not in the cache so it will try UDD and fail (no network),
        // but fetch_version_for_url returns early on error without caching.
        // On second call it would retry, which is acceptable for network errors.
        let version = cache
            .get_version_for_url("https://example.com/nonexistent.git")
            .await;
        assert_eq!(version, None);
    }

    #[tokio::test]
    #[ignore] // requires network access to UDD
    async fn test_fetch_from_udd() {
        let mut cache = VcsWatchCache::new(crate::udd::shared_pool());
        let version = cache
            .get_version_for_url("https://salsa.debian.org/python-team/packages/dulwich.git")
            .await;
        assert!(version.is_some(), "dulwich should be tracked in vcswatch");
    }
}
