//! Enrich a SCIP index's bug and CVE symbols with live documentation.

use crate::bugs;
use crate::changelog::hover::{debian_bug_markdown, launchpad_bug_markdown};
use crate::cve;
use crate::scip::symbols;
use scip::types::Index;

/// Upgrade bug and CVE symbols in the index to live documentation.
///
/// The indexer emits a static link for each referenced Debian/Launchpad bug and
/// CVE; this looks each one up (reusing the same caches and rendering the LSP
/// hover uses) and replaces the documentation with a rich summary when data is
/// found. References that aren't found keep their static link. Launchpad lookups
/// are no-ops unless the `launchpad` feature is enabled. CVE per-release status
/// is scoped to the index's source package.
pub async fn attach(index: &mut Index) {
    let pool = crate::udd::shared_pool();
    let cache = bugs::new_shared_bug_cache(pool.clone());
    let cve_cache = cve::new_shared_cve_cache(pool);
    let source = source_name(index);

    for sym in &mut index.external_symbols {
        if let Some(id) = symbols::parse_bts_bug(&sym.symbol) {
            if let Some(summary) = bugs::debian_bug_summary(&cache, id).await {
                sym.documentation = vec![debian_bug_markdown(&summary)];
            }
        } else if let Some(id) = symbols::parse_lp_bug(&sym.symbol) {
            if let Some(summary) = bugs::launchpad_bug_summary(&cache, id).await {
                sym.documentation = vec![launchpad_bug_markdown(&summary)];
            }
        } else if let Some(id) = symbols::parse_cve(&sym.symbol) {
            if let Some(summary) = cve::cve_summary(&cve_cache, &id, &source).await {
                sym.documentation = vec![cve::summary_markdown(&summary)];
            }
        }
    }
}

/// Recover the source package name from the index, for scoping CVE lookups.
///
/// Reads the package name off any `scip-debian` symbol's `Package`; falls back
/// to an empty string, which simply yields no per-release status.
fn source_name(index: &Index) -> String {
    index
        .documents
        .iter()
        .flat_map(|d| {
            d.symbols
                .iter()
                .map(|s| s.symbol.as_str())
                .chain(d.occurrences.iter().map(|o| o.symbol.as_str()))
        })
        .find_map(|sym| {
            let parsed = scip::symbol::parse_symbol(sym).ok()?;
            (parsed.scheme == symbols::SCHEME && !parsed.package.name.is_empty())
                .then(|| parsed.package.name.clone())
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use scip::types::Document;

    #[test]
    fn source_name_reads_package_off_a_debian_symbol() {
        let index = Index {
            documents: vec![Document {
                occurrences: vec![scip::types::Occurrence {
                    symbol: symbols::changelog_version("hello", "2.10-3"),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(source_name(&index), "hello");
    }

    #[test]
    fn source_name_empty_when_no_debian_symbol() {
        let index = Index {
            documents: vec![Document {
                occurrences: vec![scip::types::Occurrence {
                    symbol: symbols::cve("CVE-2024-1234"),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(source_name(&index), "");
    }
}
