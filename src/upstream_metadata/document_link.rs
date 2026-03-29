//! Document link support for debian/upstream/metadata files.
//!
//! Provides clickable links for URL-valued fields like Repository,
//! Bug-Database, Documentation, etc.

use tower_lsp_server::ls_types::{DocumentLink, Position, Range, Uri};
use yaml_edit::{Document, YamlNode};

/// Field names whose values are URLs that should be clickable links.
const URL_FIELD_NAMES: &[&str] = &[
    "Repository",
    "Repository-Browse",
    "Bug-Database",
    "Bug-Submit",
    "Changelog",
    "Documentation",
    "FAQ",
    "Donation",
    "Gallery",
    "Webservice",
];

fn is_url_field(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    URL_FIELD_NAMES
        .iter()
        .any(|f| f.to_ascii_lowercase() == lower)
}

/// Get document links for URL-valued fields in an upstream/metadata file.
pub fn get_document_links(doc: &Document, source_text: &str) -> Vec<DocumentLink> {
    let mapping = match doc.as_mapping() {
        Some(m) => m,
        None => return vec![],
    };

    let mut links = vec![];

    for entry in mapping.entries() {
        let key_scalar = match entry.key_node() {
            Some(YamlNode::Scalar(s)) => s,
            _ => continue,
        };

        let key_text = key_scalar.as_string();
        if !is_url_field(&key_text) {
            continue;
        }

        let val_scalar = match entry.value_node() {
            Some(YamlNode::Scalar(s)) => s,
            _ => continue,
        };

        let val_text = val_scalar.as_string();
        let uri = match val_text.parse::<Uri>() {
            Ok(uri) => uri,
            Err(_) => continue,
        };

        // yaml_edit positions are 1-indexed; LSP positions are 0-indexed
        let start = val_scalar.start_position(source_text);
        let end = val_scalar.end_position(source_text);
        let range = Range::new(
            Position::new(start.line as u32 - 1, start.column as u32 - 1),
            Position::new(end.line as u32 - 1, end.column as u32 - 1),
        );

        links.push(DocumentLink {
            range,
            target: Some(uri),
            tooltip: Some(key_text),
            data: None,
        });
    }

    links
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_doc(text: &str) -> Document {
        text.parse::<Document>().unwrap()
    }

    #[test]
    fn test_url_field_produces_link() {
        let text = "Repository: https://github.com/example/project\n";
        let doc = parse_doc(text);
        let links = get_document_links(&doc, text);

        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].target.as_ref().unwrap().as_str(),
            "https://github.com/example/project"
        );
        assert_eq!(links[0].tooltip.as_deref(), Some("Repository"));
        // Value starts after "Repository: " (column 12)
        assert_eq!(links[0].range.start, Position::new(0, 12));
    }

    #[test]
    fn test_non_url_field_no_link() {
        let text = "Name: my-project\n";
        let doc = parse_doc(text);
        let links = get_document_links(&doc, text);
        assert_eq!(links.len(), 0);
    }

    #[test]
    fn test_multiple_url_fields() {
        let text = "Repository: https://github.com/example/project\nBug-Database: https://bugs.example.com\n";
        let doc = parse_doc(text);
        let links = get_document_links(&doc, text);
        assert_eq!(links.len(), 2);
    }

    #[test]
    fn test_invalid_url_skipped() {
        let text = "Repository: not a valid url\n";
        let doc = parse_doc(text);
        let links = get_document_links(&doc, text);
        assert_eq!(links.len(), 0);
    }

    #[test]
    fn test_unknown_field_no_link() {
        let text = "X-Custom: https://example.com\n";
        let doc = parse_doc(text);
        let links = get_document_links(&doc, text);
        assert_eq!(links.len(), 0);
    }

    #[test]
    fn test_mixed_url_and_text_fields() {
        let text = "Name: my-project\nRepository: https://github.com/example\nContact: maintainer@example.com\nBug-Submit: https://bugs.example.com/new\n";
        let doc = parse_doc(text);
        let links = get_document_links(&doc, text);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].tooltip.as_deref(), Some("Repository"));
        assert_eq!(links[1].tooltip.as_deref(), Some("Bug-Submit"));
    }

    #[test]
    fn test_non_mapping_document() {
        let text = "- item1\n- item2\n";
        let doc = parse_doc(text);
        let links = get_document_links(&doc, text);
        assert_eq!(links.len(), 0);
    }
}
