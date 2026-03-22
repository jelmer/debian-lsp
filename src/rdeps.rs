//! Reverse dependency counts from UDD (Ultimate Debian Database).
//!
//! Queries the `depends` table to count how many packages depend on a given package.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

/// Thread-safe shared cache for reverse dependency lookups.
pub type SharedRdepsCache = Arc<RwLock<RdepsCache>>;

/// Cached reverse dependency counts from UDD.
pub struct RdepsCache {
    pool: crate::udd::SharedPool,
    /// Map from package name to reverse dependency count. `None` means "looked up, not found".
    count_by_package: HashMap<String, Option<u32>>,
}

#[derive(sqlx::FromRow)]
struct RdepsRow {
    count: Option<i64>,
}

impl RdepsCache {
    /// Create a new reverse dependencies cache using the given UDD connection pool.
    pub fn new(pool: crate::udd::SharedPool) -> Self {
        Self {
            pool,
            count_by_package: HashMap::new(),
        }
    }

    /// Look up the reverse dependency count for a package.
    ///
    /// Returns `None` if the package is not found or the query fails.
    pub async fn get_rdeps_count(&mut self, package: &str) -> Option<u32> {
        if !self.count_by_package.contains_key(package) {
            self.fetch_rdeps_count(package).await;
        }
        self.count_by_package
            .get(package)
            .and_then(|v| v.as_ref())
            .copied()
    }

    async fn fetch_rdeps_count(&mut self, package: &str) {
        let row: Option<RdepsRow> = match sqlx::query_as(
            "SELECT COUNT(DISTINCT source) AS count FROM all_packages \
             WHERE depends LIKE '%' || $1 || '%' AND release = 'sid'",
        )
        .bind(package)
        .fetch_optional(&*self.pool)
        .await
        {
            Ok(row) => row,
            Err(e) => {
                tracing::warn!(package, error = %e, "UDD rdeps query failed");
                return;
            }
        };

        match row {
            Some(RdepsRow { count: Some(n) }) => {
                self.count_by_package
                    .insert(package.to_string(), u32::try_from(n).ok());
            }
            _ => {
                self.count_by_package.insert(package.to_string(), None);
            }
        }
    }

    /// Insert a cached entry for testing purposes.
    #[cfg(test)]
    pub(crate) fn insert_cached(&mut self, package: &str, count: u32) {
        self.count_by_package
            .insert(package.to_string(), Some(count));
    }

    /// Insert a cached "not found" entry for testing purposes.
    #[cfg(test)]
    pub(crate) fn insert_cached_missing(&mut self, package: &str) {
        self.count_by_package.insert(package.to_string(), None);
    }
}

/// Create a new shared reverse dependencies cache.
pub fn new_shared_rdeps_cache(pool: crate::udd::SharedPool) -> SharedRdepsCache {
    Arc::new(RwLock::new(RdepsCache::new(pool)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_rdeps_count_from_cache() {
        let mut cache = RdepsCache::new(crate::udd::shared_pool());
        cache.insert_cached("libc6", 15000);

        let count = cache.get_rdeps_count("libc6").await;
        assert_eq!(count, Some(15000));
    }

    #[tokio::test]
    async fn test_get_rdeps_count_unknown_package() {
        let mut cache = RdepsCache::new(crate::udd::shared_pool());
        cache.insert_cached_missing("nonexistent-package-xyz");

        let count = cache.get_rdeps_count("nonexistent-package-xyz").await;
        assert_eq!(count, None);
    }
}
