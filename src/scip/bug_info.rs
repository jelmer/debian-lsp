//! Enrich a SCIP index's bug symbols with live documentation.

use crate::bugs;
use crate::changelog::hover::{debian_bug_markdown, launchpad_bug_markdown};
use crate::scip::symbols;
use scip::types::Index;

/// Upgrade bug symbols in the index to live documentation.
///
/// The indexer emits a static link for each referenced Debian or Launchpad
/// bug; this looks each one up (reusing the same bug cache and rendering the
/// LSP hover uses) and replaces the documentation with a rich summary when the
/// bug is found. Bugs that aren't found keep their static link. Launchpad
/// lookups are no-ops unless the `launchpad` feature is enabled.
pub async fn attach(index: &mut Index) {
    let cache = bugs::new_shared_bug_cache(crate::udd::shared_pool());

    for sym in &mut index.external_symbols {
        if let Some(id) = symbols::parse_bts_bug(&sym.symbol) {
            if let Some(summary) = bugs::debian_bug_summary(&cache, id).await {
                sym.documentation = vec![debian_bug_markdown(&summary)];
            }
        } else if let Some(id) = symbols::parse_lp_bug(&sym.symbol) {
            if let Some(summary) = bugs::launchpad_bug_summary(&cache, id).await {
                sym.documentation = vec![launchpad_bug_markdown(&summary)];
            }
        }
    }
}
