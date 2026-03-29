//! Maintainer identity suggestions from environment and UDD.
//!
//! Provides completions for Maintainer and Uploaders fields by combining:
//! 1. The user's identity from `$DEBEMAIL`/`$DEBFULLNAME` (matching `dch` behavior)
//! 2. Common maintainer identities from the UDD `sources` table

use std::sync::Arc;

use tokio::sync::RwLock;

/// Thread-safe shared cache for maintainer identity lookups.
pub type SharedMaintainerCache = Arc<RwLock<MaintainerCache>>;

/// Cached maintainer identities from UDD.
pub struct MaintainerCache {
    pool: crate::udd::SharedPool,
    /// Distinct maintainer identities fetched from UDD, or `None` if not yet fetched.
    maintainers: Option<Vec<String>>,
}

#[derive(sqlx::FromRow)]
struct MaintainerRow {
    maintainer: String,
}

impl MaintainerCache {
    /// Create a new maintainer cache using the given UDD connection pool.
    pub fn new(pool: crate::udd::SharedPool) -> Self {
        Self {
            pool,
            maintainers: None,
        }
    }

    /// Get the list of known maintainer identities, fetching from UDD if needed.
    pub async fn get_maintainers(&mut self) -> &[String] {
        if self.maintainers.is_none() {
            self.fetch_maintainers().await;
        }
        self.maintainers.as_deref().unwrap_or(&[])
    }

    async fn fetch_maintainers(&mut self) {
        let rows: Vec<MaintainerRow> = match sqlx::query_as(
            "SELECT DISTINCT maintainer FROM sources \
             WHERE release = 'sid' \
             ORDER BY maintainer \
             LIMIT 5000",
        )
        .fetch_all(&*self.pool)
        .await
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!(error = %e, "UDD maintainer query failed");
                self.maintainers = Some(Vec::new());
                return;
            }
        };

        self.maintainers = Some(rows.into_iter().map(|r| r.maintainer).collect());
    }

    /// Insert cached entries for testing purposes.
    #[cfg(test)]
    pub(crate) fn insert_cached(&mut self, maintainers: Vec<String>) {
        self.maintainers = Some(maintainers);
    }
}

/// Create a new shared maintainer cache.
pub fn new_shared_maintainer_cache(pool: crate::udd::SharedPool) -> SharedMaintainerCache {
    Arc::new(RwLock::new(MaintainerCache::new(pool)))
}

/// Get the user's identity from Debian environment variables.
///
/// Checks `$DEBFULLNAME` and `$DEBEMAIL` first (matching `dch` behavior),
/// then falls back to `$EMAIL` for the email part.
pub fn get_user_identity() -> Option<String> {
    let name = std::env::var("DEBFULLNAME").ok().filter(|s| !s.is_empty());
    let email = std::env::var("DEBEMAIL")
        .ok()
        .or_else(|| std::env::var("EMAIL").ok())
        .filter(|s| !s.is_empty());

    match (name, email) {
        (Some(n), Some(e)) => Some(format!("{} <{}>", n, e)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_maintainers_from_cache() {
        let mut cache = MaintainerCache::new(crate::udd::shared_pool());
        cache.insert_cached(vec![
            "Alice <alice@example.com>".to_string(),
            "Bob <bob@example.com>".to_string(),
        ]);

        let maintainers = cache.get_maintainers().await;
        assert_eq!(maintainers.len(), 2);
        assert_eq!(maintainers[0], "Alice <alice@example.com>");
    }

    #[tokio::test]
    async fn test_get_maintainers_empty_before_fetch() {
        let mut cache = MaintainerCache::new(crate::udd::shared_pool());
        cache.insert_cached(vec![]);
        let maintainers = cache.get_maintainers().await;
        assert!(maintainers.is_empty());
    }
}
