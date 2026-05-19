//! Lazy, cached loader for the multiarch-hints feed.
//!
//! One [`HintsStore`] is shared across the language server. The first
//! request triggers a background refresh; subsequent requests serve the
//! cached parse until the next refresh. Refreshes go through
//! `multiarch_hints::cache_download_multiarch_hints_async`, which
//! conditional-GETs against the local on-disk cache so a warm cache
//! avoids the network round-trip entirely.

use std::sync::Arc;

use multiarch_hints::{cache_download_multiarch_hints_async, parse_multiarch_hints, Hint};
use tokio::sync::{Mutex, OnceCell};

/// Shared store for the parsed multiarch-hints list.
///
/// Cheap to clone: the only field is an `Arc`. The loaded list itself
/// is also stored behind an `Arc<Vec<Hint>>` so callers can move an
/// owned handle into a `spawn_blocking` task without cloning the data.
#[derive(Clone, Default)]
pub struct HintsStore {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    /// Populated on first successful load. Once set, never cleared — a
    /// subsequent refresh failure leaves the previous list in place
    /// rather than wiping a working dataset.
    hints: OnceCell<Arc<Vec<Hint>>>,
    /// Serialises in-flight refresh attempts so a flurry of concurrent
    /// requests doesn't fan out to N parallel downloads.
    refresh: Mutex<()>,
}

impl HintsStore {
    /// Returns the loaded hints, fetching and parsing them if this is
    /// the first call. Subsequent calls are O(1).
    ///
    /// Returns `None` if the download or parse fails — the error is
    /// logged but not propagated, since the caller (an LSP request
    /// handler) has nothing useful to do with it.
    pub async fn get(&self) -> Option<Arc<Vec<Hint>>> {
        if let Some(hints) = self.inner.hints.get() {
            return Some(hints.clone());
        }
        let _guard = self.inner.refresh.lock().await;
        if let Some(hints) = self.inner.hints.get() {
            return Some(hints.clone());
        }
        match Self::fetch_and_parse().await {
            Ok(parsed) => {
                let arc = Arc::new(parsed);
                let _ = self.inner.hints.set(arc.clone());
                Some(arc)
            }
            Err(e) => {
                tracing::warn!("Failed to load multiarch hints: {}", e);
                None
            }
        }
    }

    async fn fetch_and_parse() -> Result<Vec<Hint>, Box<dyn std::error::Error + Send + Sync>> {
        let bytes = cache_download_multiarch_hints_async(None).await?;
        let parsed = parse_multiarch_hints(&bytes)?;
        Ok(parsed)
    }
}
