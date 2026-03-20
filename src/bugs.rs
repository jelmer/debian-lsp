use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

mod debbugs;
mod launchpad;

pub use debbugs::DebbugsBugSummary;
pub use launchpad::LaunchpadBugSummary;

/// Thread-safe shared cache for bug tracker lookups.
pub type SharedBugCache = Arc<RwLock<BugCache>>;

#[cfg(feature = "launchpad")]
const LAUNCHPAD_CONSUMER_KEY: &str = "debian-lsp";

/// Cached bug data used by changelog completions.
pub struct BugCache {
    pool: crate::udd::SharedPool,
    bug_ids_by_package: HashMap<String, Vec<u32>>,
    bug_details_by_id: HashMap<u32, CachedDebbugsBugDetails>,
    #[cfg(feature = "launchpad")]
    launchpad_client: launchpadlib::r#async::Client,
    #[cfg(feature = "launchpad")]
    launchpad_bug_ids_by_package: HashMap<String, Vec<u32>>,
    #[cfg(feature = "launchpad")]
    launchpad_bug_details_by_id: HashMap<u32, CachedLaunchpadBugDetails>,
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
            #[cfg(feature = "launchpad")]
            launchpad_client: launchpadlib::r#async::Client::anonymous(LAUNCHPAD_CONSUMER_KEY),
            #[cfg(feature = "launchpad")]
            launchpad_bug_ids_by_package: HashMap::new(),
            #[cfg(feature = "launchpad")]
            launchpad_bug_details_by_id: HashMap::new(),
        }
    }
}

/// Create a new shared cache for bug data from UDD.
pub fn new_shared_bug_cache(pool: crate::udd::SharedPool) -> SharedBugCache {
    Arc::new(RwLock::new(BugCache::new(pool)))
}
