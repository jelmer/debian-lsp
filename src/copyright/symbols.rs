//! Document symbol generation for Debian copyright files.

use debian_copyright::lossless::Parse;
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{DocumentSymbol, SymbolKind};

use crate::position::text_range_to_lsp_range;

/// Generate document symbols for a copyright file.
///
/// The header paragraph becomes a top-level symbol, and each Files and
/// standalone License paragraph becomes a symbol, giving breadcrumb and
/// outline navigation.
#[allow(deprecated)] // DocumentSymbol::deprecated field
pub fn generate_document_symbols(parse: &Parse, source_text: &str) -> Vec<DocumentSymbol> {
    let copyright = parse.to_copyright();
    let mut symbols = Vec::new();

    if let Some(header) = copyright.header() {
        let para = header.as_deb822();
        let range = text_range_to_lsp_range(source_text, para.syntax().text_range());
        let name = "Header".to_string();

        symbols.push(DocumentSymbol {
            name,
            detail: header.format_string(),
            kind: SymbolKind::NAMESPACE,
            tags: None,
            deprecated: None,
            range,
            selection_range: range,
            children: None,
        });
    }

    for files_para in copyright.iter_files() {
        let para = files_para.as_deb822();
        let range = text_range_to_lsp_range(source_text, para.syntax().text_range());
        let files = files_para.files();
        let name = format!("Files: {}", files.join(", "));

        let detail = files_para
            .license()
            .and_then(|l| l.name().map(|s| s.to_string()));

        symbols.push(DocumentSymbol {
            name,
            detail,
            kind: SymbolKind::FILE,
            tags: None,
            deprecated: None,
            range,
            selection_range: range,
            children: None,
        });
    }

    for license_para in copyright.iter_licenses() {
        let para = license_para.as_deb822();
        let range = text_range_to_lsp_range(source_text, para.syntax().text_range());
        let name = match license_para.name() {
            Some(n) => format!("License: {n}"),
            None => "License".to_string(),
        };

        symbols.push(DocumentSymbol {
            name,
            detail: None,
            kind: SymbolKind::KEY,
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

    fn parse(text: &str) -> Parse {
        Parse::parse_relaxed(text)
    }

    #[test]
    fn test_header_only() {
        let text = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: foo\n";
        let parsed = parse(text);
        let symbols = generate_document_symbols(&parsed, text);

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Header");
        assert_eq!(
            symbols[0].detail.as_deref(),
            Some("https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/")
        );
        assert_eq!(symbols[0].kind, SymbolKind::NAMESPACE);
    }

    #[test]
    fn test_header_without_upstream_name() {
        let text = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n";
        let parsed = parse(text);
        let symbols = generate_document_symbols(&parsed, text);

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Header");
    }

    #[test]
    fn test_files_paragraphs() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/*
Copyright: 2024 Alice
License: MIT

Files: debian/*
Copyright: 2024 Bob
License: GPL-2+
";
        let parsed = parse(text);
        let symbols = generate_document_symbols(&parsed, text);

        assert_eq!(symbols.len(), 3);
        assert_eq!(symbols[0].name, "Header");
        assert_eq!(symbols[0].kind, SymbolKind::NAMESPACE);
        assert_eq!(symbols[1].name, "Files: src/*");
        assert_eq!(symbols[1].detail.as_deref(), Some("MIT"));
        assert_eq!(symbols[1].kind, SymbolKind::FILE);
        assert_eq!(symbols[2].name, "Files: debian/*");
        assert_eq!(symbols[2].detail.as_deref(), Some("GPL-2+"));
        assert_eq!(symbols[2].kind, SymbolKind::FILE);
    }

    #[test]
    fn test_license_paragraphs() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Test
License: MIT

License: MIT
 Permission is hereby granted...
";
        let parsed = parse(text);
        let symbols = generate_document_symbols(&parsed, text);

        assert_eq!(symbols.len(), 3);
        assert_eq!(symbols[2].name, "License: MIT");
        assert_eq!(symbols[2].kind, SymbolKind::KEY);
    }

    #[test]
    fn test_multiple_file_patterns() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/* lib/* include/*
Copyright: 2024 Test
License: Apache-2.0
";
        let parsed = parse(text);
        let symbols = generate_document_symbols(&parsed, text);

        assert_eq!(symbols[1].name, "Files: src/*, lib/*, include/*");
    }

    #[test]
    fn test_empty_file() {
        let text = "";
        let parsed = parse(text);
        let symbols = generate_document_symbols(&parsed, text);

        assert_eq!(symbols.len(), 0);
    }
}
