use std::num::NonZeroUsize;
use std::sync::Arc;

use lru::LruCache;
use tokio::sync::RwLock;

mod debbugs;
mod launchpad;

pub use debbugs::DebbugsBugSummary;
pub use launchpad::LaunchpadBugSummary;

/// Thread-safe shared cache for bug tracker lookups.
pub type SharedBugCache = Arc<RwLock<BugCache>>;

#[cfg(feature = "launchpad")]
const LAUNCHPAD_CONSUMER_KEY: &str = "debian-lsp";

/// Maximum number of distinct package keys cached. Each entry stores
/// a `Vec<u32>` of bug IDs (typically <100), so the cap is generous;
/// the LRU exists to bound long-session growth, not steady-state size.
const BUG_IDS_CACHE_CAPACITY: usize = 1024;

/// Maximum number of bug detail records cached. Each record holds a
/// handful of `Option<String>` fields. Bugs are referenced repeatedly
/// once seen, so the cap is sized to comfortably hold every bug from
/// a few hundred packages without evictions.
const BUG_DETAILS_CACHE_CAPACITY: usize = 32_768;

/// Cached bug data used by changelog completions.
pub struct BugCache {
    pub pool: crate::udd::SharedPool,
    bug_ids_by_package: LruCache<String, Vec<u32>>,
    bug_details_by_id: LruCache<u32, CachedDebbugsBugDetails>,
    /// Last UDD connection error, set by fetch methods and drained by callers
    /// that want to surface it (e.g. via `window/showMessage`).
    pub last_udd_error: Option<String>,
    #[cfg(feature = "launchpad")]
    launchpad_client: launchpadlib::r#async::Client,
    #[cfg(feature = "launchpad")]
    launchpad_bug_ids_by_package: LruCache<String, Vec<u32>>,
    #[cfg(feature = "launchpad")]
    launchpad_bug_details_by_id: LruCache<u32, CachedLaunchpadBugDetails>,
}

/// Cached details for a single Debian bug report.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CachedDebbugsBugDetails {
    title: Option<String>,
    severity: Option<String>,
    done: bool,
    tags: Option<String>,
    forwarded: Option<String>,
    originator: Option<String>,
}

/// Cached details for a Launchpad bug relevant to completion.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(feature = "launchpad")]
struct CachedLaunchpadBugDetails {
    title: Option<String>,
    status: Option<String>,
    done: bool,
}

#[derive(sqlx::FromRow)]
pub struct BugRow {
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
            bug_ids_by_package: LruCache::new(
                NonZeroUsize::new(BUG_IDS_CACHE_CAPACITY).expect("non-zero capacity"),
            ),
            bug_details_by_id: LruCache::new(
                NonZeroUsize::new(BUG_DETAILS_CACHE_CAPACITY).expect("non-zero capacity"),
            ),
            last_udd_error: None,
            #[cfg(feature = "launchpad")]
            launchpad_client: launchpadlib::r#async::Client::anonymous(LAUNCHPAD_CONSUMER_KEY),
            #[cfg(feature = "launchpad")]
            launchpad_bug_ids_by_package: LruCache::new(
                NonZeroUsize::new(BUG_IDS_CACHE_CAPACITY).expect("non-zero capacity"),
            ),
            #[cfg(feature = "launchpad")]
            launchpad_bug_details_by_id: LruCache::new(
                NonZeroUsize::new(BUG_DETAILS_CACHE_CAPACITY).expect("non-zero capacity"),
            ),
        }
    }
}

/// Create a new shared cache for bug data from UDD.
pub fn new_shared_bug_cache(pool: crate::udd::SharedPool) -> SharedBugCache {
    Arc::new(RwLock::new(BugCache::new(pool)))
}

/// Look up a Debian bug summary, fetching from UDD on a cache miss.
///
/// Avoids holding the cache lock across the network call: it checks the
/// in-memory cache first; if absent, clones the pool, drops the lock, queries
/// UDD, then re-acquires to insert.
pub async fn debian_bug_summary(cache: &SharedBugCache, id: u32) -> Option<DebbugsBugSummary> {
    // Fast path: already cached.
    if let Some(s) = cache.write().await.get_cached_debian_bug_summary(id) {
        return Some(s);
    }
    // Slow path: fetch from UDD. The write guard is dropped here before the
    // network await so other callers aren't blocked.
    let pool = cache.read().await.pool.clone();
    let row = BugCache::query_bug_by_id(&pool, id).await?;
    let mut guard = cache.write().await;
    guard.insert_bug_row(id, row);
    guard.get_cached_debian_bug_summary(id)
}
