//! Index `debian/source/options` and `debian/source/local-options` into SCIP.
//!
//! The file format is one dpkg-source option per line, optionally with a value
//! after `=` (`compression = xz`), and `#` comments. Each option name emits a
//! cross-package symbol so archive-wide search can find every package setting a
//! given option.

use crate::scip::linetable::LineTable;
use crate::scip::symbols;
use crate::source_options::fields;
use scip::types::{Document, Occurrence, SymbolRole};

/// Indexed result for a source options file.
pub struct SourceOptionsIndex {
    /// The SCIP document.
    pub document: Document,
    /// Option names referenced in the file, for archive-wide documentation.
    pub options: Vec<String>,
}

/// Parse and index `debian/source/options` or `debian/source/local-options`.
pub fn index(text: &str, relative_path: &str) -> SourceOptionsIndex {
    let lines = LineTable::new(text);
    let mut occurrences = Vec::new();
    let mut options = Vec::new();

    let mut offset = 0u32;
    for line in text.split_inclusive('\n') {
        let line_len = line.len() as u32;
        let content = line.trim_end_matches(['\n', '\r']);

        if let Some(hash) = content.find('#') {
            let start = offset + hash as u32;
            let end = offset + content.len() as u32;
            occurrences.push(Occurrence {
                range: lines.range(start, end),
                syntax_kind: scip::types::SyntaxKind::Comment.into(),
                ..Default::default()
            });
            offset += line_len;
            continue;
        }

        let trimmed = content.trim();
        if trimmed.is_empty() {
            offset += line_len;
            continue;
        }

        let eq = content.find('=');
        let name = match eq {
            Some(eq) => content[..eq].trim(),
            None => trimmed,
        };

        if !name.is_empty() {
            // Locate the option name's byte span within the original line.
            let name_start = offset + content.find(name).unwrap_or(0) as u32;
            let name_end = name_start + name.len() as u32;
            let sym = symbols::source_option(name);
            occurrences.push(Occurrence {
                range: lines.range(name_start, name_end),
                symbol: sym.clone(),
                symbol_roles: SymbolRole::Definition as i32,
                syntax_kind: scip::types::SyntaxKind::Identifier.into(),
                ..Default::default()
            });
            options.push(name.to_owned());
        }

        // Highlight the value after `=` (symbol-less, highlight only).
        if let Some(eq) = eq {
            let after_eq = &content[eq + 1..];
            let value = after_eq.trim();
            if !value.is_empty() {
                let value_start =
                    offset + eq as u32 + 1 + (after_eq.len() - after_eq.trim_start().len()) as u32;
                let value_end = value_start + value.len() as u32;
                occurrences.push(Occurrence {
                    range: lines.range(value_start, value_end),
                    syntax_kind: scip::types::SyntaxKind::StringLiteral.into(),
                    ..Default::default()
                });
            }
        }

        offset += line_len;
    }

    SourceOptionsIndex {
        document: Document {
            language: "plain".to_owned(),
            relative_path: relative_path.to_owned(),
            text: text.to_owned(),
            occurrences,
            symbols: Vec::new(),
            position_encoding: scip::types::PositionEncoding::UTF8CodeUnitOffsetFromLineStart
                .into(),
            ..Default::default()
        },
        options,
    }
}

/// Documentation for a dpkg-source option symbol, if the option is known.
pub fn option_documentation(name: &str) -> Option<String> {
    fields::find_option(name).map(|opt| opt.description.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_boolean_option() {
        let idx = index("single-debian-patch\n", "debian/source/options");
        assert_eq!(idx.document.occurrences.len(), 1);
        assert!(idx.document.occurrences[0]
            .symbol
            .contains("single-debian-patch"));
        assert_eq!(idx.document.occurrences[0].range, vec![0, 0, 0, 19]);
        assert_eq!(idx.options, vec!["single-debian-patch".to_owned()]);
    }

    #[test]
    fn indexes_option_with_value() {
        let idx = index("compression = xz\n", "debian/source/options");
        assert_eq!(idx.document.occurrences.len(), 2);
        // The option name carries the symbol; only its own span, not the value.
        assert!(idx.document.occurrences[0].symbol.contains("compression"));
        assert_eq!(idx.document.occurrences[0].range, vec![0, 0, 0, 11]);
        // The value is highlighted but symbol-less.
        assert!(idx.document.occurrences[1].symbol.is_empty());
        assert_eq!(
            idx.document.occurrences[1].syntax_kind,
            scip::types::SyntaxKind::StringLiteral.into()
        );
        assert_eq!(idx.document.occurrences[1].range, vec![0, 14, 0, 16]);
        assert_eq!(idx.options, vec!["compression".to_owned()]);
    }

    #[test]
    fn boolean_option_has_no_value_occurrence() {
        let idx = index("single-debian-patch\n", "debian/source/options");
        assert_eq!(idx.document.occurrences.len(), 1);
    }

    #[test]
    fn indexes_comment() {
        let idx = index(
            "# set compression\ncompression = xz\n",
            "debian/source/options",
        );
        // comment, option name, value.
        assert_eq!(idx.document.occurrences.len(), 3);
        assert_eq!(
            idx.document.occurrences[0].syntax_kind,
            scip::types::SyntaxKind::Comment.into()
        );
        assert_eq!(idx.document.occurrences[0].range, vec![0, 0, 0, 17]);
        assert_eq!(idx.options, vec!["compression".to_owned()]);
    }

    #[test]
    fn empty_file_emits_nothing() {
        let idx = index("", "debian/source/options");
        assert_eq!(idx.document.occurrences.len(), 0);
        assert!(idx.options.is_empty());
    }

    #[test]
    fn blank_lines_skipped() {
        let idx = index(
            "compression = xz\n\nsingle-debian-patch\n",
            "debian/source/options",
        );
        // compression name, xz value, single-debian-patch name.
        assert_eq!(idx.document.occurrences.len(), 3);
        assert_eq!(
            idx.options,
            vec!["compression".to_owned(), "single-debian-patch".to_owned()]
        );
    }

    #[test]
    fn documentation_for_known_option() {
        assert_eq!(
            option_documentation("single-debian-patch"),
            Some("Use debian/patches/debian-changes as automatic patch".to_owned())
        );
        assert_eq!(option_documentation("not-a-real-option"), None);
    }
}
