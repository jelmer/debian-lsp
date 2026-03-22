//! Popularity contest data from UDD (Ultimate Debian Database).
//!
//! Queries the `popcon` table to find install counts for packages.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

/// Thread-safe shared cache for popcon lookups.
pub type SharedPopconCache = Arc<RwLock<PopconCache>>;

/// Cached popcon data from UDD.
pub struct PopconCache {
    pool: crate::udd::SharedPool,
    /// Map from package name to install count. `None` means "looked up, not found".
    inst_by_package: HashMap<String, Option<u32>>,
}

#[derive(sqlx::FromRow)]
struct PopconRow {
    insts: Option<i32>,
}

impl PopconCache {
    /// Create a new popcon cache using the given UDD connection pool.
    pub fn new(pool: crate::udd::SharedPool) -> Self {
        Self {
            pool,
            inst_by_package: HashMap::new(),
        }
    }

    /// Look up the install count for a package.
    ///
    /// Returns `None` if the package is not found in popcon or the query fails.
    pub async fn get_inst_count(&mut self, package: &str) -> Option<u32> {
        if !self.inst_by_package.contains_key(package) {
            self.fetch_inst_count(package).await;
        }
        self.inst_by_package
            .get(package)
            .and_then(|v| v.as_ref())
            .copied()
    }

    async fn fetch_inst_count(&mut self, package: &str) {
        let row: Option<PopconRow> =
            match sqlx::query_as("SELECT insts FROM popcon WHERE package = $1 LIMIT 1")
                .bind(package)
                .fetch_optional(&*self.pool)
                .await
            {
                Ok(row) => row,
                Err(e) => {
                    tracing::warn!(package, error = %e, "UDD popcon query failed");
                    return;
                }
            };

        match row {
            Some(PopconRow { insts: Some(n) }) => {
                self.inst_by_package
                    .insert(package.to_string(), u32::try_from(n).ok());
            }
            _ => {
                self.inst_by_package.insert(package.to_string(), None);
            }
        }
    }

    /// Insert a cached entry for testing purposes.
    #[cfg(test)]
    pub(crate) fn insert_cached(&mut self, package: &str, inst_count: u32) {
        self.inst_by_package
            .insert(package.to_string(), Some(inst_count));
    }

    /// Insert a cached "not found" entry for testing purposes.
    #[cfg(test)]
    pub(crate) fn insert_cached_missing(&mut self, package: &str) {
        self.inst_by_package.insert(package.to_string(), None);
    }
}

/// Create a new shared popcon cache.
pub fn new_shared_popcon_cache(pool: crate::udd::SharedPool) -> SharedPopconCache {
    Arc::new(RwLock::new(PopconCache::new(pool)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_inst_count_from_cache() {
        let mut cache = PopconCache::new(crate::udd::shared_pool());
        cache.insert_cached("hello", 42000);

        let count = cache.get_inst_count("hello").await;
        assert_eq!(count, Some(42000));
    }

    #[tokio::test]
    async fn test_get_inst_count_unknown_package() {
        let mut cache = PopconCache::new(crate::udd::shared_pool());
        cache.insert_cached_missing("nonexistent-package-xyz");

        let count = cache.get_inst_count("nonexistent-package-xyz").await;
        assert_eq!(count, None);
    }
}
