//! Document symbols for DEP-3 patch headers.
//!
//! Each header field becomes a symbol so the outline view shows the
//! patch's metadata at a glance. The diff body is left untouched.

use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{DocumentSymbol, SymbolKind};

use crate::position::text_range_to_lsp_range;

/// Generate document symbols for a DEP-3 header. `header` is the
/// parsed deb822 of the header portion only; `source_text` is the
/// whole patch buffer, used to map rowan byte ranges back to LSP
/// `Range`s. One symbol per field; the field name is the symbol name
/// and the field value is its `detail` (truncated for multi-line
/// values).
#[allow(deprecated)] // DocumentSymbol::deprecated is required by the LSP type
pub fn generate_document_symbols(
    header: &deb822_lossless::Deb822,
    source_text: &str,
) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();
    let Some(paragraph) = header.paragraphs().next() else {
        return symbols;
    };
    for entry in paragraph.entries() {
        let Some(name) = entry.key() else {
            continue;
        };
        let entry_range = entry.syntax().text_range();
        let range = text_range_to_lsp_range(source_text, entry_range);
        let detail = entry.value().as_str().to_string();
        let detail = first_line_truncated(&detail, 80);
        symbols.push(DocumentSymbol {
            name: name.to_string(),
            detail: Some(detail),
            kind: SymbolKind::FIELD,
            tags: None,
            deprecated: None,
            range,
            selection_range: range,
            children: None,
        });
    }
    symbols
}

/// Return the first line of `s`, trimmed and clipped to `max_chars`.
/// Used so multi-line `Description:` values don't blow up the symbol
/// detail line.
fn first_line_truncated(s: &str, max_chars: usize) -> String {
    let line = s.split('\n').next().unwrap_or(s).trim();
    if line.chars().count() <= max_chars {
        return line.to_string();
    }
    let mut out: String = line.chars().take(max_chars).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> Vec<DocumentSymbol> {
        let header_end = dep3::lossless::header_end(text);
        let parsed = deb822_lossless::Deb822::parse(&text[..header_end]);
        generate_document_symbols(&parsed.tree(), text)
    }

    #[test]
    fn one_symbol_per_field() {
        let symbols = run("Author: alice\nDescription: bla\nForwarded: not-needed\n");
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["Author", "Description", "Forwarded"]);
    }

    #[test]
    fn detail_carries_first_line_of_value() {
        let symbols = run("Description: short synopsis\n long body line\n more\n");
        assert_eq!(symbols[0].detail.as_deref(), Some("short synopsis"));
    }

    #[test]
    fn diff_body_not_inspected() {
        let symbols = run("Author: alice\n---\n@@ -1 +1 @@\n-x\n+y\n");
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["Author"]);
    }

    #[test]
    fn empty_header_returns_no_symbols() {
        // File starts with a diff marker — no header at all.
        assert!(run("---\n@@ -1 +1 @@\n").is_empty());
    }
}
