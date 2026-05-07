//! Document symbols for DEP-3 patch headers.
//!
//! Each header field becomes a symbol so the outline view shows the
//! patch's metadata at a glance. The diff body is left untouched.

use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{DocumentSymbol, SymbolKind};

use crate::position::text_range_to_lsp_range;

/// Generate document symbols for the DEP-3 header at the top of
/// `source_text`. One symbol per field; the field name is the symbol
/// name and the field value is its `detail` (truncated for multi-line
/// values).
#[allow(deprecated)] // DocumentSymbol::deprecated is required by the LSP type
pub fn generate_document_symbols(source_text: &str) -> Vec<DocumentSymbol> {
    let header_end = dep3::lossless::header_end(source_text);
    if header_end == 0 {
        return Vec::new();
    }
    let header_text = &source_text[..header_end];
    let parsed = deb822_lossless::Deb822::parse(header_text);
    let deb822 = parsed.tree();
    let mut symbols = Vec::new();
    let Some(paragraph) = deb822.paragraphs().next() else {
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

    #[test]
    fn one_symbol_per_field() {
        let text = "Author: alice\nDescription: bla\nForwarded: not-needed\n";
        let symbols = generate_document_symbols(text);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["Author", "Description", "Forwarded"]);
    }

    #[test]
    fn detail_carries_first_line_of_value() {
        let text = "Description: short synopsis\n long body line\n more\n";
        let symbols = generate_document_symbols(text);
        assert_eq!(symbols[0].detail.as_deref(), Some("short synopsis"));
    }

    #[test]
    fn diff_body_not_inspected() {
        let text = "Author: alice\n---\n@@ -1 +1 @@\n-x\n+y\n";
        let symbols = generate_document_symbols(text);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["Author"]);
    }

    #[test]
    fn empty_header_returns_no_symbols() {
        // File starts with a diff marker — no header at all.
        let text = "---\n@@ -1 +1 @@\n";
        assert!(generate_document_symbols(text).is_empty());
    }
}
