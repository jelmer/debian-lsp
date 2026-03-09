use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use super::fields::UPSTREAM_FIELDS;

/// Get completions for a debian/upstream/metadata file at the given position.
///
/// If the cursor is at the start of a line (column 0), returns field name
/// completions. Otherwise returns empty.
pub fn get_completions(source_text: &str, position: Position) -> Vec<CompletionItem> {
    if position.character != 0 {
        return vec![];
    }

    // Check if the cursor is on an empty line or at the start of a new line
    let lines: Vec<&str> = source_text.lines().collect();
    let line = lines.get(position.line as usize).copied().unwrap_or("");

    // Only offer field completions on empty or whitespace-only lines
    if !line.trim().is_empty() {
        return vec![];
    }

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
    fn test_get_completions_not_at_column_zero() {
        let text = "Repository: https://example.com\n";
        let completions = get_completions(text, Position::new(0, 12));
        assert_eq!(completions.len(), 0);
    }

    #[test]
    fn test_get_completions_on_existing_field() {
        let text = "Repository: https://example.com\n";
        let completions = get_completions(text, Position::new(0, 0));
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
