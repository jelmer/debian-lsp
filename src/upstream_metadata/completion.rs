use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use super::fields::UPSTREAM_FIELDS;

/// Get completions for a debian/upstream/metadata file at the given position.
///
/// Returns field name completions when the cursor is at the start of a line
/// (empty line or typing a top-level YAML key that hasn't been completed with
/// a colon yet).
pub fn get_completions(source_text: &str, position: Position) -> Vec<CompletionItem> {
    let lines: Vec<&str> = source_text.lines().collect();
    let line = lines.get(position.line as usize).copied().unwrap_or("");

    // Offer field completions when:
    // 1. The line is empty/whitespace (cursor at column 0)
    // 2. The cursor is on a line that looks like an incomplete top-level key
    //    (no colon yet, no leading whitespace — i.e. not a nested value)
    if line.trim().is_empty() {
        if position.character == 0 {
            return get_field_completions();
        }
        return vec![];
    }

    // If the line already has a colon, the user is editing a value, not a key
    if line.contains(':') {
        return vec![];
    }

    // If the line starts with whitespace, it's a nested/continuation value
    if line.starts_with(' ') || line.starts_with('\t') {
        return vec![];
    }

    // The user is typing a top-level key name — offer field completions
    get_field_completions()
}

/// Generate field name completions for all known DEP-12 fields.
fn get_field_completions() -> Vec<CompletionItem> {
    UPSTREAM_FIELDS
        .iter()
        .map(|field| CompletionItem {
            label: field.name.to_string(),
            kind: Some(CompletionItemKind::FIELD),
            detail: Some(field.description.to_string()),
            insert_text: Some(format!("{}: ", field.name)),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_completions_at_start_of_empty_line() {
        let text = "Repository: https://example.com\n\n";
        let completions = get_completions(text, Position::new(1, 0));

        assert_eq!(completions.len(), UPSTREAM_FIELDS.len());
        assert_eq!(completions[0].label, "Repository");
        assert_eq!(
            completions[0].detail.as_deref(),
            Some("URL of the upstream source repository")
        );
    }

    #[test]
    fn test_get_completions_on_value() {
        let text = "Repository: https://example.com\n";
        let completions = get_completions(text, Position::new(0, 12));
        assert_eq!(completions.len(), 0);
    }

    #[test]
    fn test_get_completions_on_existing_field_key() {
        // Line has a colon, so the field name is already complete
        let text = "Repository: https://example.com\n";
        let completions = get_completions(text, Position::new(0, 0));
        assert_eq!(completions.len(), 0);
    }

    #[test]
    fn test_get_completions_typing_field_name() {
        let text = "Repository: https://example.com\nBug";
        let completions = get_completions(text, Position::new(1, 3));
        assert_eq!(completions.len(), UPSTREAM_FIELDS.len());
    }

    #[test]
    fn test_get_completions_partial_field_name_at_col_zero() {
        let text = "Repository: https://example.com\nBug";
        let completions = get_completions(text, Position::new(1, 0));
        assert_eq!(completions.len(), UPSTREAM_FIELDS.len());
    }

    #[test]
    fn test_get_completions_on_indented_line() {
        let text = "Reference:\n  - https://example.com\n";
        let completions = get_completions(text, Position::new(1, 4));
        assert_eq!(completions.len(), 0);
    }

    #[test]
    fn test_get_completions_indented_line() {
        // Indented lines are continuation/value lines — no field completions
        let text = "Repository: https://example.com\n  indented\n";
        let completions = get_completions(text, Position::new(1, 2));
        assert_eq!(completions.len(), 0);
    }

    #[test]
    fn test_get_completions_empty_file() {
        let completions = get_completions("", Position::new(0, 0));
        assert_eq!(completions.len(), UPSTREAM_FIELDS.len());
    }

    #[test]
    fn test_get_completions_line_beyond_file() {
        let text = "Repository: https://example.com\n";
        let completions = get_completions(text, Position::new(5, 0));
        // Line 5 doesn't exist, defaults to empty string which is whitespace-only
        assert_eq!(completions.len(), UPSTREAM_FIELDS.len());
    }

    #[test]
    fn test_field_completions_have_insert_text() {
        let completions = get_field_completions();
        assert_eq!(completions.len(), UPSTREAM_FIELDS.len());
        for c in &completions {
            assert_eq!(c.kind, Some(CompletionItemKind::FIELD));
            assert!(c.insert_text.as_ref().unwrap().ends_with(": "));
            assert!(c.detail.is_some());
        }
    }
}
