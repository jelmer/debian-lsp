//! Index `debian/upstream/metadata` (YAML) into a SCIP document.
//!
//! Walks the top-level mapping of the first YAML document and emits a
//! definition occurrence for each scalar key. Nested mappings and sequences
//! are not indexed.

use crate::scip::linetable::LineTable;
use crate::scip::symbols;
use scip::types::{Document, Occurrence, SymbolInformation, SymbolRole};
use yaml_edit::{Parse, YamlFile};

/// Indexed result for `debian/upstream/metadata`.
pub struct UpstreamMetadataIndex {
    /// The SCIP document.
    pub document: Document,
}

/// Parse and index `debian/upstream/metadata`.
pub fn index(
    text: &str,
    relative_path: &str,
    source: &str,
    version: Option<&str>,
) -> UpstreamMetadataIndex {
    let lines = LineTable::new(text);
    let mut occurrences: Vec<Occurrence> = Vec::new();
    let mut symbols_info: Vec<SymbolInformation> = Vec::new();

    let mut url_symbols_seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let yaml = Parse::<YamlFile>::parse_yaml(text).tree();
    if let Some(mapping) = yaml.document().and_then(|d| d.as_mapping()) {
        for entry in mapping.entries() {
            let Some(key_node) = entry.key_node() else {
                continue;
            };
            let Some(scalar) = key_node.as_scalar() else {
                continue;
            };
            let key = scalar.as_string();
            if key.is_empty() {
                continue;
            }
            let range = scalar.byte_range();
            let sym = symbols::upstream_metadata_field(source, version, &key);
            occurrences.push(Occurrence {
                range: lines.range(range.start, range.end),
                symbol: sym.clone(),
                symbol_roles: SymbolRole::Definition as i32,
                syntax_kind: scip::types::SyntaxKind::IdentifierAttribute.into(),
                ..Default::default()
            });
            symbols_info.push(SymbolInformation {
                symbol: sym,
                kind: scip::types::symbol_information::Kind::Field.into(),
                display_name: key.clone(),
                documentation: crate::upstream_metadata::fields::field_description(&key)
                    .map(|(_, desc)| desc.to_owned())
                    .into_iter()
                    .collect(),
                ..Default::default()
            });

            // Clickable link for a URL-valued field (Repository, Bug-Database,
            // Homepage, ...). The whole scalar value is the URL.
            if !crate::upstream_metadata::document_link::is_url_field(&key) {
                continue;
            }
            let Some(value_node) = entry.value_node() else {
                continue;
            };
            let Some(value_scalar) = value_node.as_scalar() else {
                continue;
            };
            let url = value_scalar.as_string();
            let url = url.trim();
            if url.is_empty() {
                continue;
            }
            let vrange = value_scalar.byte_range();
            crate::scip::links::emit_url(
                url,
                vrange.start,
                vrange.end,
                &lines,
                &mut occurrences,
                &mut symbols_info,
                &mut url_symbols_seen,
            );
        }
    }

    UpstreamMetadataIndex {
        document: Document {
            language: "yaml".to_owned(),
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
# Comment line
Repository: https://github.com/example/hello
Bug-Database: https://github.com/example/hello/issues
Archive: GitHub
Reference:
  - Author: Doe
    Title: Paper
";

    #[test]
    fn indexes_toplevel_keys() {
        let idx = index(SAMPLE, "debian/upstream/metadata", "hello", Some("2.10-3"));
        let defs: Vec<_> = idx
            .document
            .occurrences
            .iter()
            .filter(|o| (o.symbol_roles & SymbolRole::Definition as i32) != 0)
            .collect();
        // Repository, Bug-Database, Archive, Reference — but not the nested
        // Author/Title.
        assert_eq!(defs.len(), 4);
        let repository = idx
            .document
            .symbols
            .iter()
            .find(|s| s.symbol.contains("Repository"))
            .expect("Repository symbol");
        // The field carries the same description the LSP hover shows.
        assert_eq!(
            repository.documentation,
            vec![
                crate::upstream_metadata::fields::field_description("Repository")
                    .unwrap()
                    .1
                    .to_owned()
            ]
        );
        assert!(!idx
            .document
            .symbols
            .iter()
            .any(|s| s.symbol.contains("Author")));
    }

    #[test]
    fn links_url_values() {
        let idx = index(SAMPLE, "debian/upstream/metadata", "hello", Some("2.10-3"));
        for url in [
            "https://github.com/example/hello",
            "https://github.com/example/hello/issues",
        ] {
            let sym = symbols::web_url(url);
            assert!(
                idx.document.occurrences.iter().any(|o| o.symbol == sym),
                "no link occurrence for {url}"
            );
            let info = idx
                .document
                .symbols
                .iter()
                .find(|s| s.symbol == sym)
                .unwrap_or_else(|| panic!("no symbol info for {url}"));
            assert_eq!(info.documentation, vec![symbols::web_url_doc(url)]);
        }
    }
}
