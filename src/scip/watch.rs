//! Index a `debian/watch` file into a SCIP document.
//!
//! Surfaces the upstream URL of each entry as a definition, so editor tooling
//! can show hover info and (eventually) navigate to upstream release pages.

use crate::scip::linetable::LineTable;
use crate::scip::symbols;
use debian_watch::linebased::WatchFile;
use scip::types::{Document, Occurrence, SymbolInformation, SymbolRole};

/// Indexed result for a `debian/watch` file.
pub struct WatchIndex {
    /// The SCIP document.
    pub document: Document,
}

/// Parse and index a `debian/watch` file.
pub fn index(text: &str, relative_path: &str, source: &str, version: Option<&str>) -> WatchIndex {
    let watch = WatchFile::from_str_relaxed(text);
    let lines = LineTable::new(text);
    let mut occurrences: Vec<Occurrence> = Vec::new();
    let mut symbols_info: Vec<SymbolInformation> = Vec::new();

    // Syntax-highlighting occurrences for the whole file.
    occurrences.extend(crate::scip::highlight::watch(text, &lines));

    for (i, entry) in watch.entries().enumerate() {
        let entry_sym =
            symbols::upstream_metadata_field(source, version, &format!("watch-entry-{}", i));
        if let Some(url_node) = entry.url_node() {
            let r = url_node.text_range();
            let s: u32 = r.start().into();
            let e: u32 = r.end().into();
            occurrences.push(Occurrence {
                range: lines.range(s, e),
                symbol: entry_sym.clone(),
                symbol_roles: SymbolRole::Definition as i32,
                ..Default::default()
            });
            symbols_info.push(SymbolInformation {
                symbol: entry_sym,
                kind: scip::types::symbol_information::Kind::Constant.into(),
                display_name: text[s as usize..e as usize].to_owned(),
                ..Default::default()
            });
        }
    }

    WatchIndex {
        document: Document {
            language: "debwatch".to_owned(),
            relative_path: relative_path.to_owned(),
            text: text.to_owned(),
            occurrences,
            symbols: symbols_info,
            position_encoding: scip::types::PositionEncoding::UTF8CodeUnitOffsetFromLineStart
                .into(),
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
version=4
opts=\"uversionmangle=s/-/./\" https://example.org/hello/ hello-(.+)\\.tar\\.gz
";

    #[test]
    fn indexes_url_definitions() {
        let idx = index(SAMPLE, "debian/watch", "hello", Some("2.10-3"));
        let defs: Vec<_> = idx
            .document
            .occurrences
            .iter()
            .filter(|o| (o.symbol_roles & SymbolRole::Definition as i32) != 0)
            .collect();
        assert_eq!(defs.len(), 1);
    }

    #[test]
    fn highlights_v5_deb822_format() {
        let text = "Version: 5\n\nSource: https://example.org/hello/\nMatching-Pattern: hello-(.+)\\.tar\\.gz\n";
        let idx = index(text, "debian/watch", "hello", Some("2.10-3"));
        // v5 (deb822) watch files still get syntax-highlighting occurrences.
        let unspecified = scip::types::SyntaxKind::UnspecifiedSyntaxKind.into();
        assert!(
            idx.document
                .occurrences
                .iter()
                .any(|o| o.symbol.is_empty() && o.syntax_kind != unspecified),
            "expected highlight occurrences for a v5 watch file"
        );
    }
}
