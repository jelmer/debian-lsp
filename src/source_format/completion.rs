use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position, Uri};

use super::detection::is_source_format_file;
use super::fields::SOURCE_FORMATS;

/// Get completion items for debian/source/format file
pub fn get_completions(_uri: &Uri, _position: Position) -> Vec<CompletionItem> {
    if !is_source_format_file(_uri) {
        return Vec::new();
    }

    SOURCE_FORMATS
        .iter()
        .map(|(format, description)| CompletionItem {
            label: (*format).to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some((*description).to_string()),
            insert_text: Some((*format).to_string()),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_completions() {
        let uri = str::parse("file:///tmp/debian/source/format").unwrap();
        let position = Position::new(0, 0);
        let completions = get_completions(&uri, position);

        assert_eq!(completions.len(), 5);
        assert!(completions
            .iter()
            .any(|c| c.label == "3.0 (quilt)" && c.kind == Some(CompletionItemKind::VALUE)));
        assert!(completions
            .iter()
            .any(|c| c.label == "3.0 (native)" && c.kind == Some(CompletionItemKind::VALUE)));
        assert!(completions
            .iter()
            .any(|c| c.label == "1.0" && c.kind == Some(CompletionItemKind::VALUE)));
    }

    #[test]
    fn test_get_completions_non_format_file() {
        let uri = str::parse("file:///tmp/debian/control").unwrap();
        let position = Position::new(0, 0);
        let completions = get_completions(&uri, position);

        assert_eq!(completions.len(), 0);
    }
}
