//! Lookup and caching of CVE details from UDD's `security_issues` tables.

use std::num::NonZeroUsize;
use std::sync::Arc;

use lru::LruCache;
use tokio::sync::RwLock;

use crate::udd::SharedPool;

/// Thread-safe shared cache for CVE lookups.
pub type SharedCveCache = Arc<RwLock<CveCache>>;

/// Maximum number of `(cve, source)` lookups cached. Each record is small; the
/// cap exists to bound long-session growth.
const CVE_CACHE_CAPACITY: usize = 8192;

/// Per-release status for a CVE in a given source package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CveReleaseStatus {
    /// Debian release codename (e.g. `bookworm`, `sid`).
    pub release: String,
    /// Status string (e.g. `resolved`, `open`, `undetermined`).
    pub status: Option<String>,
    /// Version the issue was fixed in for this release, if any.
    pub fixed_version: Option<String>,
}

/// Details for a CVE, drawn from UDD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CveSummary {
    /// The CVE identifier, e.g. `CVE-2024-1234`.
    pub id: String,
    /// Free-text description of the vulnerability.
    pub description: Option<String>,
    /// Scope of the issue (e.g. `local`, `remote`).
    pub scope: Option<String>,
    /// Linked Debian bug number, if any.
    pub bug: Option<u32>,
    /// Per-release status for the source package, ordered by release name.
    pub releases: Vec<CveReleaseStatus>,
}

/// Cache of CVE summaries keyed by `(cve-id, source-package)`.
///
/// The source package qualifies the key because the per-release status is
/// specific to a source; the description and scope are the same across sources.
pub struct CveCache {
    pool: SharedPool,
    by_key: LruCache<(String, String), Option<CveSummary>>,
}

impl CveCache {
    /// Create a new cache using the given UDD connection pool.
    pub fn new(pool: SharedPool) -> Self {
        Self {
            pool,
            by_key: LruCache::new(
                NonZeroUsize::new(CVE_CACHE_CAPACITY).expect("non-zero capacity"),
            ),
        }
    }
}

/// Create a new shared CVE cache.
pub fn new_shared_cve_cache(pool: SharedPool) -> SharedCveCache {
    Arc::new(RwLock::new(CveCache::new(pool)))
}

#[derive(sqlx::FromRow)]
struct IssueRow {
    description: Option<String>,
    scope: Option<String>,
    bug: Option<i32>,
}

#[derive(sqlx::FromRow)]
struct ReleaseRow {
    release: String,
    status: Option<String>,
    fixed_version: Option<String>,
}

/// Look up a CVE summary scoped to `source`, fetching from UDD on a cache miss.
///
/// `source` scopes the per-release status; pass the source package the CVE is
/// referenced from. Returns `None` when the CVE is unknown to UDD (or the
/// lookup fails), in which case callers should fall back to a plain link.
pub async fn cve_summary(cache: &SharedCveCache, id: &str, source: &str) -> Option<CveSummary> {
    let key = (id.to_owned(), source.to_owned());

    // Fast path: already cached (including a cached negative result).
    if let Some(cached) = cache.write().await.by_key.get(&key) {
        return cached.clone();
    }

    // Slow path: fetch from UDD with the lock released.
    let pool = cache.read().await.pool.clone();
    let summary = fetch_cve(&pool, id, source).await;

    cache.write().await.by_key.put(key, summary.clone());
    summary
}

/// Query UDD for a single CVE's details scoped to a source package.
async fn fetch_cve(pool: &SharedPool, id: &str, source: &str) -> Option<CveSummary> {
    let issue: Option<IssueRow> = match sqlx::query_as(
        "SELECT description, scope::text AS scope, bug \
         FROM security_issues WHERE issue = $1 \
         ORDER BY (source = $2) DESC LIMIT 1",
    )
    .bind(id)
    .bind(source)
    .fetch_optional(&**pool)
    .await
    {
        Ok(row) => row,
        Err(e) => {
            tracing::warn!(cve = id, error = %e, "UDD CVE query failed");
            return None;
        }
    };

    let issue = issue?;

    let releases: Vec<ReleaseRow> = match sqlx::query_as(
        "SELECT release, status::text AS status, fixed_version \
         FROM security_issues_releases WHERE issue = $1 AND source = $2 \
         ORDER BY release",
    )
    .bind(id)
    .bind(source)
    .fetch_all(&**pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(cve = id, error = %e, "UDD CVE release query failed");
            Vec::new()
        }
    };

    Some(CveSummary {
        id: id.to_owned(),
        description: issue.description,
        scope: issue.scope,
        bug: issue.bug.and_then(|b| u32::try_from(b).ok()),
        releases: releases
            .into_iter()
            .map(|r| CveReleaseStatus {
                release: r.release,
                status: r.status,
                fixed_version: r.fixed_version,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cached_negative_result_is_returned() {
        let cache = new_shared_cve_cache(crate::udd::shared_pool());
        cache
            .write()
            .await
            .by_key
            .put(("CVE-2099-0001".to_owned(), "foo".to_owned()), None);
        assert_eq!(cve_summary(&cache, "CVE-2099-0001", "foo").await, None);
    }

    #[tokio::test]
    async fn cached_summary_is_returned_without_network() {
        let cache = new_shared_cve_cache(crate::udd::shared_pool());
        let summary = CveSummary {
            id: "CVE-2021-44228".to_owned(),
            description: Some("Log4Shell".to_owned()),
            scope: Some("remote".to_owned()),
            bug: Some(1001478),
            releases: vec![CveReleaseStatus {
                release: "bookworm".to_owned(),
                status: Some("resolved".to_owned()),
                fixed_version: Some("2.15.0-1".to_owned()),
            }],
        };
        cache.write().await.by_key.put(
            ("CVE-2021-44228".to_owned(), "apache-log4j2".to_owned()),
            Some(summary.clone()),
        );
        assert_eq!(
            cve_summary(&cache, "CVE-2021-44228", "apache-log4j2").await,
            Some(summary)
        );
    }

    #[tokio::test]
    #[ignore] // requires network access to UDD
    async fn fetches_known_cve_from_udd() {
        let cache = new_shared_cve_cache(crate::udd::shared_pool());
        let summary = cve_summary(&cache, "CVE-2021-44228", "apache-log4j2")
            .await
            .expect("CVE-2021-44228 should be known to UDD");
        assert_eq!(summary.id, "CVE-2021-44228");
        assert!(summary.description.is_some());
        assert!(
            !summary.releases.is_empty(),
            "apache-log4j2 should have release statuses"
        );
    }
}
