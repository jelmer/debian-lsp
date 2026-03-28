//! Hover support for debian/upstream/metadata files.

use tower_lsp_server::ls_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};
use yaml_edit::{Document, YamlNode};

use super::fields::{get_standard_field_name, UPSTREAM_FIELDS};

fn get_field_description(field_name: &str) -> Option<(&'static str, &'static str)> {
    let lowercase = field_name.to_lowercase();
    UPSTREAM_FIELDS
        .iter()
        .find(|f| f.name.to_lowercase() == lowercase)
        .map(|f| (f.name, f.description))
}

fn make_hover(field_name: &str, description: &str) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("**{}**\n\n{}", field_name, description),
        }),
        range: None,
    }
}

/// Get hover information for a debian/upstream/metadata file at the given cursor position.
pub fn get_hover(doc: &Document, source_text: &str, position: Position) -> Option<Hover> {
    let mapping = doc.as_mapping()?;

    // Convert LSP 0-indexed position to yaml_edit 1-indexed LineColumn
    let target_line = position.line as usize + 1;
    let target_col = position.character as usize + 1;

    for entry in mapping.entries() {
        // Check if cursor is on the key
        if let Some(YamlNode::Scalar(key_scalar)) = entry.key_node() {
            let key_text = key_scalar.as_string();
            let start = key_scalar.start_position(source_text);
            let end = key_scalar.end_position(source_text);

            if target_line >= start.line
                && target_line <= end.line
                && (target_line != start.line || target_col >= start.column)
                && (target_line != end.line || target_col <= end.column)
            {
                return get_field_description(&key_text)
                    .map(|(canonical, desc)| make_hover(canonical, desc));
            }

            // Check if cursor is on the value
            if let Some(YamlNode::Scalar(val_scalar)) = entry.value_node() {
                let val_start = val_scalar.start_position(source_text);
                let val_end = val_scalar.end_position(source_text);

                if target_line >= val_start.line
                    && target_line <= val_end.line
                    && (target_line != val_start.line || target_col >= val_start.column)
                    && (target_line != val_end.line || target_col <= val_end.column)
                {
                    let canonical = get_standard_field_name(&key_text).unwrap_or(&key_text);
                    return get_field_description(canonical).map(|(c, desc)| make_hover(c, desc));
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hover_markdown(hover: &Hover) -> &str {
        match &hover.contents {
            HoverContents::Markup(markup) => &markup.value,
            _ => panic!("Expected markup content"),
        }
    }

    fn parse_doc(text: &str) -> Document {
        text.parse::<Document>().unwrap()
    }

    #[test]
    fn test_hover_on_field_key() {
        let text = "Repository: https://github.com/example/project\n";
        let doc = parse_doc(text);
        let hover = get_hover(&doc, text, Position::new(0, 3)).unwrap();
        assert_eq!(
            hover_markdown(&hover),
            "**Repository**\n\nURL of the upstream source repository"
        );
    }

    #[test]
    fn test_hover_on_field_value() {
        let text = "Repository: https://github.com/example/project\n";
        let doc = parse_doc(text);
        let hover = get_hover(&doc, text, Position::new(0, 15)).unwrap();
        assert_eq!(
            hover_markdown(&hover),
            "**Repository**\n\nURL of the upstream source repository"
        );
    }

    #[test]
    fn test_hover_on_unknown_field() {
        let text = "X-Custom: value\n";
        let doc = parse_doc(text);
        assert_eq!(get_hover(&doc, text, Position::new(0, 3)), None);
    }

    #[test]
    fn test_hover_on_empty_line() {
        let text = "Repository: https://example.com\n\nBug-Database: https://bugs.example.com\n";
        let doc = parse_doc(text);
        assert_eq!(get_hover(&doc, text, Position::new(1, 0)), None);
    }

    #[test]
    fn test_hover_case_insensitive_key() {
        let text = "repository: https://example.com\n";
        let doc = parse_doc(text);
        let hover = get_hover(&doc, text, Position::new(0, 3)).unwrap();
        assert_eq!(
            hover_markdown(&hover),
            "**Repository**\n\nURL of the upstream source repository"
        );
    }

    #[test]
    fn test_hover_second_field() {
        let text = "Repository: https://example.com\nBug-Database: https://bugs.example.com\n";
        let doc = parse_doc(text);
        let hover = get_hover(&doc, text, Position::new(1, 5)).unwrap();
        assert_eq!(
            hover_markdown(&hover),
            "**Bug-Database**\n\nURL of the upstream bug tracking system"
        );
    }
}
