//! Enrich a SCIP index's Debian BTS bug symbols with live documentation.

use crate::bugs;
use crate::changelog::hover::debian_bug_markdown;
use crate::scip::symbols;
use scip::types::Index;

/// Upgrade BTS bug symbols in the index to live documentation.
///
/// The indexer emits a static link for each referenced Debian bug; this looks
/// each one up in UDD (reusing the same bug cache the LSP hover uses) and
/// replaces the documentation with a rich summary when the bug is found. Bugs
/// that aren't found keep their static link.
pub async fn attach(index: &mut Index) {
    let cache = bugs::new_shared_bug_cache(crate::udd::shared_pool());

    for sym in &mut index.external_symbols {
        let Some(id) = symbols::parse_bts_bug(&sym.symbol) else {
            continue;
        };
        if let Some(summary) = bugs::debian_bug_summary(&cache, id).await {
            sym.documentation = vec![debian_bug_markdown(&summary)];
        }
    }
}
