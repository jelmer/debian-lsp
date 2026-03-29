use lintian_overrides::{LintianOverrides, SyntaxKind};
use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use crate::position::try_position_to_offset;

/// Get completion items for a lintian overrides file.
pub fn get_completions(
    source_text: &str,
    position: Position,
    tags: &[(String, String)],
) -> Vec<CompletionItem> {
    let offset = match try_position_to_offset(source_text, position) {
        Some(o) => o,
        None => return Vec::new(),
    };

    let parsed = LintianOverrides::parse(source_text);
    let root = parsed.syntax();

    // Find the token at the cursor position
    let token = root.token_at_offset(offset).right_biased();

    // Determine context: are we on a tag, package spec, or starting a new line?
    let on_tag = token.as_ref().is_some_and(|t| t.kind() == SyntaxKind::TAG);
    let on_package_name = token
        .as_ref()
        .is_some_and(|t| t.kind() == SyntaxKind::PACKAGE_NAME);

    // If on a tag or at the start of a line (where a tag would go), suggest known tags
    if on_tag || on_package_name {
        return tags
            .iter()
            .map(|(tag, description)| CompletionItem {
                label: tag.clone(),
                kind: Some(CompletionItemKind::VALUE),
                detail: if description.is_empty() {
                    None
                } else {
                    Some(description.clone())
                },
                ..Default::default()
            })
            .collect();
    }

    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completions_empty_file() {
        let completions = get_completions("", Position::new(0, 0), &[]);
        assert!(completions.is_empty());
    }

    #[test]
    fn test_completions_on_tag() {
        let text = "some-tag\n";
        let tags = vec![
            ("some-tag".to_string(), "A test tag".to_string()),
            ("other-tag".to_string(), "Another tag".to_string()),
        ];
        // Position at start of line, on the tag token
        let completions = get_completions(text, Position::new(0, 0), &tags);
        assert_eq!(completions.len(), 2);
        assert!(completions.iter().any(|c| c.label == "some-tag"));
        assert!(completions.iter().any(|c| c.label == "other-tag"));
    }

    #[test]
    fn test_completions_no_tags_available() {
        let text = "some-tag\n";
        let completions = get_completions(text, Position::new(0, 0), &[]);
        assert!(completions.is_empty());
    }
}
