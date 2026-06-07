//! Index `debian/source/format` into a SCIP document.
//!
//! The file is a single line carrying the source format string
//! (e.g. `3.0 (quilt)`). The emitted symbol is cross-package so archive-wide
//! search can find every package using a given format.

use crate::scip::linetable::LineTable;
use crate::scip::symbols;
use scip::types::{Document, Occurrence, SymbolInformation, SymbolRole};

/// Indexed result for `debian/source/format`.
pub struct SourceFormatIndex {
    /// The SCIP document.
    pub document: Document,
}

/// Parse and index `debian/source/format`.
pub fn index(text: &str, relative_path: &str) -> SourceFormatIndex {
    let lines = LineTable::new(text);
    let mut occurrences = Vec::new();
    let mut symbols_info = Vec::new();

    let trimmed = text.trim_end();
    if !trimmed.is_empty() {
        let sym = symbols::source_format(trimmed);
        occurrences.push(Occurrence {
            range: lines.range(0, trimmed.len() as u32),
            symbol: sym.clone(),
            symbol_roles: SymbolRole::Definition as i32,
            syntax_kind: scip::types::SyntaxKind::StringLiteral.into(),
            ..Default::default()
        });
        symbols_info.push(SymbolInformation {
            symbol: sym,
            kind: scip::types::symbol_information::Kind::Type.into(),
            display_name: trimmed.to_owned(),
            documentation: crate::source_format::fields::format_description(trimmed)
                .map(str::to_owned)
                .into_iter()
                .collect(),
            ..Default::default()
        });
    }

    SourceFormatIndex {
        document: Document {
            language: "plain".to_owned(),
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

    #[test]
    fn indexes_format_string() {
        let idx = index("3.0 (quilt)\n", "debian/source/format");
        assert_eq!(idx.document.occurrences.len(), 1);
        assert!(idx.document.occurrences[0].symbol.contains("3.0 (quilt)"));
        assert_eq!(idx.document.symbols.len(), 1);
        assert_eq!(
            idx.document.symbols[0].documentation,
            vec!["Source format with quilt-based patches (recommended)"]
        );
    }

    #[test]
    fn empty_file_emits_nothing() {
        let idx = index("", "debian/source/format");
        assert_eq!(idx.document.occurrences.len(), 0);
    }
}
