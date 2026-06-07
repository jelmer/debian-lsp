//! Index a `debian/changelog` file into SCIP documents.

use crate::scip::linetable::LineTable;
use crate::scip::symbols;
use debian_changelog::bugs::{iter_bug_refs, Bug};
use debian_changelog::{ChangeLog, SyntaxKind};
use rowan::ast::AstNode;
use scip::types::{Document, Occurrence, SymbolInformation, SymbolRole, SyntaxKind as ScipSyntax};
use std::collections::BTreeSet;

/// Indexed result for `debian/changelog`.
pub struct ChangelogIndex {
    /// The SCIP document.
    pub document: Document,
    /// The source package name as declared in the topmost (newest) entry.
    pub source_name: Option<String>,
    /// The version of the topmost entry, as a string.
    pub topmost_version: Option<String>,
    /// Debian BTS bug numbers referenced anywhere in the changelog.
    pub bug_numbers: BTreeSet<u32>,
}

/// Parse and index a `debian/changelog` file.
pub fn index(text: &str, relative_path: &str) -> ChangelogIndex {
    let cl = ChangeLog::parse_relaxed(text);
    let lines = LineTable::new(text);
    let mut occurrences: Vec<Occurrence> = Vec::new();

    // Syntax-highlighting occurrences for the whole file.
    occurrences.extend(crate::scip::highlight::changelog(&cl, &lines));
    let mut symbols_info: Vec<SymbolInformation> = Vec::new();
    let mut source_name: Option<String> = None;
    let mut topmost_version: Option<String> = None;
    let mut bug_numbers: BTreeSet<u32> = BTreeSet::new();

    for (i, entry) in cl.iter().enumerate() {
        let pkg = entry.package();
        let ver = entry.try_version().and_then(|r| r.ok());
        let ver_string = ver.as_ref().map(|v| v.to_string());

        if i == 0 {
            source_name = pkg.clone();
            topmost_version.clone_from(&ver_string);
        }

        if let (Some(p), Some(v)) = (pkg.as_deref(), ver_string.as_deref()) {
            let sym = symbols::changelog_version(p, v);
            if let Some(vr) = entry.version_range() {
                occurrences.push(Occurrence {
                    range: lines.range(vr.start().into(), vr.end().into()),
                    symbol: sym.clone(),
                    symbol_roles: SymbolRole::Definition as i32,
                    ..Default::default()
                });
            }
            symbols_info.push(SymbolInformation {
                symbol: sym,
                kind: scip::types::symbol_information::Kind::Constant.into(),
                display_name: v.to_owned(),
                ..Default::default()
            });
        }

        // Identity (maintainer) reference from the footer.
        if let (Some(addr), Some(r)) = (entry.email(), entry.email_range()) {
            if !addr.is_empty() {
                occurrences.push(Occurrence {
                    range: lines.range(r.start().into(), r.end().into()),
                    symbol: symbols::identity(&addr),
                    ..Default::default()
                });
            }
        }

        // Bug references inside detail tokens.
        for tok in entry.syntax().descendants_with_tokens() {
            let Some(token) = tok.as_token() else {
                continue;
            };
            if token.kind() != SyntaxKind::DETAIL {
                continue;
            }
            let detail_text = token.text();
            let detail_start = u32::from(token.text_range().start());
            // Emit one occurrence per individual bug number. Only Debian BTS
            // bugs get a symbol; Launchpad references are skipped. The
            // occurrence carries both the symbol (for hover/navigation) and a
            // numeric syntax kind (so SCIP consumers highlight the number).
            for bug_ref in iter_bug_refs(detail_text) {
                let Bug::Debian(n) = bug_ref.bug else {
                    continue;
                };
                let abs_start = detail_start + bug_ref.start as u32;
                let abs_end = detail_start + bug_ref.end as u32;
                bug_numbers.insert(n);
                occurrences.push(Occurrence {
                    range: lines.range(abs_start, abs_end),
                    symbol: symbols::bts_bug(&n.to_string()),
                    syntax_kind: ScipSyntax::NumericLiteral.into(),
                    ..Default::default()
                });
            }
        }
    }

    ChangelogIndex {
        document: Document {
            language: "debchangelog".to_owned(),
            relative_path: relative_path.to_owned(),
            text: text.to_owned(),
            occurrences,
            symbols: symbols_info,
            position_encoding: scip::types::PositionEncoding::UTF8CodeUnitOffsetFromLineStart
                .into(),
            ..Default::default()
        },
        source_name,
        topmost_version,
        bug_numbers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
hello (2.10-3) unstable; urgency=medium

  * Fix segfault on empty input. (Closes: #999888)
  * Another change. (LP: #1234567)

 -- Jelmer Vernooĳ <jelmer@debian.org>  Tue, 27 May 2026 12:00:00 +0000

hello (2.10-2) unstable; urgency=medium

  * Previous release.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Mon, 26 May 2026 12:00:00 +0000
";

    #[test]
    fn indexes_versions_and_debian_bugs() {
        let idx = index(SAMPLE, "debian/changelog");
        assert_eq!(idx.source_name.as_deref(), Some("hello"));
        assert_eq!(idx.topmost_version.as_deref(), Some("2.10-3"));

        // Two version definitions + one Debian BTS reference (Launchpad ignored).
        let defs: Vec<_> = idx
            .document
            .occurrences
            .iter()
            .filter(|o| (o.symbol_roles & SymbolRole::Definition as i32) != 0)
            .collect();
        assert_eq!(defs.len(), 2);

        let bug_refs: Vec<_> = idx
            .document
            .occurrences
            .iter()
            .filter(|o| o.symbol.starts_with("scip-debian-bts"))
            .collect();
        assert_eq!(bug_refs.len(), 1, "expected one Debian BTS ref");

        // The bug reference is reported as the set of referenced numbers and
        // is highlighted as a numeric literal.
        assert_eq!(idx.bug_numbers, BTreeSet::from([999888]));
        assert_eq!(
            bug_refs[0].syntax_kind,
            ScipSyntax::NumericLiteral.into(),
            "bug number should be highlighted"
        );
    }
}
