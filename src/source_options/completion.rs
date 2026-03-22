use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position, Uri};

use super::detection::{is_source_local_options_file, is_source_options_or_local_options_file};
use super::fields::{COMPRESSION_LEVEL_VALUES, COMPRESSION_VALUES, SOURCE_OPTIONS};

/// Get completion items for debian/source/options or debian/source/local-options file
pub fn get_completions(uri: &Uri, position: Position, source_text: &str) -> Vec<CompletionItem> {
    if !is_source_options_or_local_options_file(uri) {
        return Vec::new();
    }

    let is_local = is_source_local_options_file(uri);

    // Find the current line text
    let line_text = source_text
        .lines()
        .nth(position.line as usize)
        .unwrap_or("");

    // Skip comment lines
    if line_text.trim_start().starts_with('#') {
        return Vec::new();
    }

    // Check if we're completing a value (after '=')
    if let Some(eq_pos) = line_text.find('=') {
        let option_name = line_text[..eq_pos].trim();
        let value_part = line_text[eq_pos + 1..].trim().trim_matches('"');

        // Only provide value completions if cursor is after the '='
        if (position.character as usize) > eq_pos {
            return get_value_completions(option_name, value_part);
        }
    }

    // Complete option names
    SOURCE_OPTIONS
        .iter()
        .filter(|opt| is_local || opt.allowed_in_options)
        .map(|opt| {
            let insert_text = if opt.takes_value {
                format!("{} = ", opt.name)
            } else {
                opt.name.to_string()
            };
            CompletionItem {
                label: opt.name.to_string(),
                kind: Some(CompletionItemKind::PROPERTY),
                detail: Some(opt.description.to_string()),
                insert_text: Some(insert_text),
                ..Default::default()
            }
        })
        .collect()
}

/// Get value completions for a specific option
fn get_value_completions(option_name: &str, _prefix: &str) -> Vec<CompletionItem> {
    let values: &[(&str, &str)] = match option_name {
        "compression" => COMPRESSION_VALUES,
        "compression-level" => COMPRESSION_LEVEL_VALUES,
        _ => return Vec::new(),
    };

    values
        .iter()
        .map(|(value, description)| CompletionItem {
            label: (*value).to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some((*description).to_string()),
            insert_text: Some((*value).to_string()),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_completions_option_names() {
        let uri = str::parse("file:///tmp/debian/source/options").unwrap();
        let position = Position::new(0, 0);
        let completions = get_completions(&uri, position, "");

        assert!(!completions.is_empty());
        assert!(completions
            .iter()
            .any(|c| c.label == "compression" && c.kind == Some(CompletionItemKind::PROPERTY)));
        assert!(completions.iter().any(|c| c.label == "single-debian-patch"));
        // abort-on-upstream-changes is local-options only
        assert!(!completions
            .iter()
            .any(|c| c.label == "abort-on-upstream-changes"));
    }

    #[test]
    fn test_get_completions_local_options_includes_all() {
        let uri = str::parse("file:///tmp/debian/source/local-options").unwrap();
        let position = Position::new(0, 0);
        let completions = get_completions(&uri, position, "");

        assert!(completions
            .iter()
            .any(|c| c.label == "abort-on-upstream-changes"));
        assert!(completions.iter().any(|c| c.label == "compression"));
    }

    #[test]
    fn test_get_completions_compression_values() {
        let uri = str::parse("file:///tmp/debian/source/options").unwrap();
        let position = Position::new(0, 15);
        let source_text = "compression = ";
        let completions = get_completions(&uri, position, source_text);

        assert_eq!(completions.len(), COMPRESSION_VALUES.len());
        assert!(completions
            .iter()
            .any(|c| c.label == "xz" && c.kind == Some(CompletionItemKind::VALUE)));
        assert!(completions.iter().any(|c| c.label == "gzip"));
    }

    #[test]
    fn test_get_completions_compression_level_values() {
        let uri = str::parse("file:///tmp/debian/source/options").unwrap();
        let position = Position::new(0, 22);
        let source_text = "compression-level = ";
        let completions = get_completions(&uri, position, source_text);

        assert_eq!(completions.len(), COMPRESSION_LEVEL_VALUES.len());
        assert!(completions.iter().any(|c| c.label == "9"));
        assert!(completions.iter().any(|c| c.label == "best"));
    }

    #[test]
    fn test_get_completions_comment_line() {
        let uri = str::parse("file:///tmp/debian/source/options").unwrap();
        let position = Position::new(0, 5);
        let source_text = "# comment";
        let completions = get_completions(&uri, position, source_text);

        assert!(completions.is_empty());
    }

    #[test]
    fn test_get_completions_non_options_file() {
        let uri = str::parse("file:///tmp/debian/control").unwrap();
        let position = Position::new(0, 0);
        let completions = get_completions(&uri, position, "");

        assert!(completions.is_empty());
    }

    #[test]
    fn test_value_options_have_equals_in_insert_text() {
        let uri = str::parse("file:///tmp/debian/source/options").unwrap();
        let position = Position::new(0, 0);
        let completions = get_completions(&uri, position, "");

        let compression = completions
            .iter()
            .find(|c| c.label == "compression")
            .unwrap();
        assert_eq!(compression.insert_text.as_deref(), Some("compression = "));

        let single_patch = completions
            .iter()
            .find(|c| c.label == "single-debian-patch")
            .unwrap();
        assert_eq!(
            single_patch.insert_text.as_deref(),
            Some("single-debian-patch")
        );
    }
}
