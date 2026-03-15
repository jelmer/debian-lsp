//! Document symbol generation for Debian control files.

use debian_control::lossless::{Control, Parse};
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{DocumentSymbol, SymbolKind};

use crate::position::text_range_to_lsp_range;

/// Generate document symbols for a control file.
///
/// The source paragraph becomes a NAMESPACE symbol and each binary
/// paragraph becomes a PACKAGE symbol, enabling breadcrumb and outline
/// navigation.
#[allow(deprecated)] // DocumentSymbol::deprecated field
pub fn generate_document_symbols(parse: &Parse<Control>, source_text: &str) -> Vec<DocumentSymbol> {
    let control = parse.tree();
    let mut symbols = Vec::new();

    if let Some(source) = control.source() {
        let para = source.as_deb822();
        let range = text_range_to_lsp_range(source_text, para.syntax().text_range());
        let name = match source.name() {
            Some(n) => format!("Source: {n}"),
            None => "Source".to_string(),
        };

        symbols.push(DocumentSymbol {
            name,
            detail: None,
            kind: SymbolKind::NAMESPACE,
            tags: None,
            deprecated: None,
            range,
            selection_range: range,
            children: None,
        });
    }

    for binary in control.binaries() {
        let para = binary.as_deb822();
        let range = text_range_to_lsp_range(source_text, para.syntax().text_range());
        let name = match binary.name() {
            Some(n) => format!("Package: {n}"),
            None => "Package".to_string(),
        };

        symbols.push(DocumentSymbol {
            name,
            detail: None,
            kind: SymbolKind::PACKAGE,
            tags: None,
            deprecated: None,
            range,
            selection_range: range,
            children: None,
        });
    }

    symbols
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_and_binary() {
        let text = "\
Source: mypackage
Maintainer: Test <test@example.com>

Package: mypackage
Architecture: any
Description: A test package
";
        let parsed = Control::parse(text);
        let symbols = generate_document_symbols(&parsed, text);

        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "Source: mypackage");
        assert_eq!(symbols[0].kind, SymbolKind::NAMESPACE);
        assert_eq!(symbols[1].name, "Package: mypackage");
        assert_eq!(symbols[1].kind, SymbolKind::PACKAGE);
    }

    #[test]
    fn test_multiple_binaries() {
        let text = "\
Source: foo
Maintainer: Test <test@example.com>

Package: foo
Architecture: any
Description: Main package

Package: foo-dev
Architecture: any
Description: Development files

Package: foo-doc
Architecture: all
Description: Documentation
";
        let parsed = Control::parse(text);
        let symbols = generate_document_symbols(&parsed, text);

        assert_eq!(symbols.len(), 4);
        assert_eq!(symbols[0].name, "Source: foo");
        assert_eq!(symbols[1].name, "Package: foo");
        assert_eq!(symbols[2].name, "Package: foo-dev");
        assert_eq!(symbols[3].name, "Package: foo-doc");
    }

    #[test]
    fn test_ranges_do_not_overlap() {
        let text = "\
Source: foo
Maintainer: Test <test@example.com>

Package: foo
Architecture: any
Description: Main

Package: foo-dev
Architecture: any
Description: Dev
";
        let parsed = Control::parse(text);
        let symbols = generate_document_symbols(&parsed, text);

        for i in 0..symbols.len() - 1 {
            assert!(
                symbols[i].range.end.line <= symbols[i + 1].range.start.line,
                "Symbol {} ({}) overlaps with {} ({})",
                i,
                symbols[i].name,
                i + 1,
                symbols[i + 1].name
            );
        }
    }

    #[test]
    fn test_empty_file() {
        let text = "";
        let parsed = Control::parse(text);
        let symbols = generate_document_symbols(&parsed, text);

        assert_eq!(symbols.len(), 0);
    }
}
