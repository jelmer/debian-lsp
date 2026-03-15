//! Document symbol generation for Debian changelog files.

use debian_changelog::{ChangeLog, Parse};
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{DocumentSymbol, SymbolKind};

use crate::position::text_range_to_lsp_range;

/// Generate document symbols for a changelog file.
///
/// Each changelog entry becomes a symbol with the package name and version
/// as its label, allowing breadcrumb navigation.
#[allow(deprecated)] // DocumentSymbol::deprecated field
pub fn generate_document_symbols(
    parse: &Parse<ChangeLog>,
    source_text: &str,
) -> Vec<DocumentSymbol> {
    let changelog = parse.tree();
    let mut symbols = Vec::new();

    for entry in changelog.entries() {
        let package = entry.package().unwrap_or_default();
        let version = entry.version().map(|v| v.to_string()).unwrap_or_default();

        let name = format!("{package} ({version})");

        let entry_range = text_range_to_lsp_range(source_text, entry.syntax().text_range());

        // The selection range is the header line (package + version)
        let selection_range = entry
            .header()
            .map(|h| text_range_to_lsp_range(source_text, h.syntax().text_range()))
            .unwrap_or(entry_range);

        symbols.push(DocumentSymbol {
            name,
            detail: entry.distributions().map(|d| d.join(" ")),
            kind: SymbolKind::PACKAGE,
            tags: None,
            deprecated: None,
            range: entry_range,
            selection_range,
            children: None,
        });
    }

    symbols
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_entry() {
        let text = "pkg (1.0-1) unstable; urgency=low\n\n  * Change.\n\n -- T <t@t.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = ChangeLog::parse(text);
        let symbols = generate_document_symbols(&parsed, text);

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "pkg (1.0-1)");
        assert_eq!(symbols[0].detail.as_deref(), Some("unstable"));
        assert_eq!(symbols[0].kind, SymbolKind::PACKAGE);
    }

    #[test]
    fn test_multiple_entries() {
        let text = "\
pkg (2.0-1) unstable; urgency=medium

  * Second release.

 -- A <a@example.com>  Mon, 01 Jan 2025 12:00:00 +0000

pkg (1.0-1) experimental; urgency=low

  * First release.

 -- B <b@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
";
        let parsed = ChangeLog::parse(text);
        let symbols = generate_document_symbols(&parsed, text);

        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "pkg (2.0-1)");
        assert_eq!(symbols[0].detail.as_deref(), Some("unstable"));
        assert_eq!(symbols[1].name, "pkg (1.0-1)");
        assert_eq!(symbols[1].detail.as_deref(), Some("experimental"));
    }

    #[test]
    fn test_selection_range_is_header() {
        let text = "pkg (1.0-1) unstable; urgency=low\n\n  * Change.\n\n -- T <t@t.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = ChangeLog::parse(text);
        let symbols = generate_document_symbols(&parsed, text);

        // Selection range should start at the header
        assert_eq!(symbols[0].selection_range.start.line, 0);
        // Entry range should span more lines than the selection range
        assert!(symbols[0].range.end.line > symbols[0].selection_range.start.line);
    }

    #[test]
    fn test_empty_changelog() {
        let text = "";
        let parsed = ChangeLog::parse(text);
        let symbols = generate_document_symbols(&parsed, text);

        assert_eq!(symbols.len(), 0);
    }

    #[test]
    fn test_multiple_distributions() {
        let text = "pkg (1.0-1) unstable testing; urgency=low\n\n  * Change.\n\n -- T <t@t.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = ChangeLog::parse(text);
        let symbols = generate_document_symbols(&parsed, text);

        assert_eq!(symbols[0].detail.as_deref(), Some("unstable testing"));
    }

    #[test]
    fn test_entry_ranges_do_not_overlap() {
        let text = "\
pkg (2.0-1) unstable; urgency=medium

  * Second.

 -- A <a@a.com>  Mon, 01 Jan 2025 12:00:00 +0000

pkg (1.0-1) unstable; urgency=low

  * First.

 -- B <b@b.com>  Mon, 01 Jan 2024 12:00:00 +0000
";
        let parsed = ChangeLog::parse(text);
        let symbols = generate_document_symbols(&parsed, text);

        assert_eq!(symbols.len(), 2);
        // First entry ends before second entry starts
        assert!(symbols[0].range.end.line <= symbols[1].range.start.line);
    }
}
