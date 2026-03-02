use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, Documentation, Position, Uri,
};

use super::detection::is_watch_file;
use super::fields::{OptionValueType, WATCH_OPTIONS, WATCH_VERSIONS};

/// Get completion items for a given position in a watch file
pub fn get_completions(uri: &Uri, _position: Position) -> Vec<CompletionItem> {
    if !is_watch_file(uri) {
        return Vec::new();
    }

    let mut completions = Vec::new();
    completions.extend(get_option_completions());
    completions.extend(get_version_completions());
    completions
}

/// Get completion items for watch file options
pub fn get_option_completions() -> Vec<CompletionItem> {
    WATCH_OPTIONS
        .iter()
        .map(|option| {
            let insert_text = match option.value_type {
                OptionValueType::Boolean => option.name.to_string(),
                OptionValueType::String | OptionValueType::Enum(_) => {
                    format!("{}=", option.name)
                }
            };

            CompletionItem {
                label: option.name.to_string(),
                kind: Some(CompletionItemKind::PROPERTY),
                detail: Some(option.description.to_string()),
                documentation: Some(Documentation::String(option.description.to_string())),
                insert_text: Some(insert_text),
                ..Default::default()
            }
        })
        .collect()
}

/// Get completion items for option values (for enum-type options)
pub fn get_option_value_completions(option_name: &str) -> Vec<CompletionItem> {
    WATCH_OPTIONS
        .iter()
        .find(|opt| opt.name == option_name)
        .and_then(|opt| {
            if let OptionValueType::Enum(values) = opt.value_type {
                Some(
                    values
                        .iter()
                        .map(|value| CompletionItem {
                            label: value.to_string(),
                            kind: Some(CompletionItemKind::VALUE),
                            detail: Some(format!("Value for {}", option_name)),
                            ..Default::default()
                        })
                        .collect(),
                )
            } else {
                None
            }
        })
        .unwrap_or_default()
}

/// Get completion items for watch file versions
pub fn get_version_completions() -> Vec<CompletionItem> {
    WATCH_VERSIONS
        .iter()
        .map(|version| CompletionItem {
            label: format!("version={}", version),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(format!("Watch file format version {}", version)),
            insert_text: Some(format!("version={}", version)),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_completions_for_watch_file() {
        let uri = str::parse("file:///path/to/debian/watch").unwrap();
        let position = Position::new(0, 0);

        let completions = get_completions(&uri, position);
        assert!(!completions.is_empty());

        // Should have both option and version completions
        let option_count = completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::PROPERTY))
            .count();
        let version_count = completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::KEYWORD))
            .count();

        assert!(option_count > 0);
        assert!(version_count > 0);
    }

    #[test]
    fn test_get_completions_for_non_watch_file() {
        let uri = str::parse("file:///path/to/other.txt").unwrap();
        let position = Position::new(0, 0);

        let completions = get_completions(&uri, position);
        assert!(completions.is_empty());
    }

    #[test]
    fn test_option_completions() {
        let completions = get_option_completions();

        assert!(!completions.is_empty());

        // Check that all completions have required properties
        for completion in &completions {
            assert!(!completion.label.is_empty());
            assert_eq!(completion.kind, Some(CompletionItemKind::PROPERTY));
            assert!(completion.detail.is_some());
            assert!(completion.documentation.is_some());
            assert!(completion.insert_text.is_some());
        }

        // Check for specific options
        let labels: Vec<_> = completions.iter().map(|c| &c.label).collect();
        assert!(labels.iter().any(|l| *l == "mode"));
        assert!(labels.iter().any(|l| *l == "pgpmode"));
        assert!(labels.iter().any(|l| *l == "uversionmangle"));
    }

    #[test]
    fn test_option_value_completions() {
        // Test enum option values
        let mode_values = get_option_value_completions("mode");
        assert!(!mode_values.is_empty());
        let mode_labels: Vec<_> = mode_values.iter().map(|c| &c.label).collect();
        assert!(mode_labels.contains(&&"lwp".to_string()));
        assert!(mode_labels.contains(&&"git".to_string()));
        assert!(mode_labels.contains(&&"svn".to_string()));

        // Test string option (should return empty)
        let string_values = get_option_value_completions("uversionmangle");
        assert!(string_values.is_empty());

        // Test unknown option (should return empty)
        let unknown_values = get_option_value_completions("nonexistent");
        assert!(unknown_values.is_empty());
    }

    #[test]
    fn test_version_completions() {
        let completions = get_version_completions();

        assert_eq!(completions.len(), WATCH_VERSIONS.len());

        for completion in &completions {
            assert!(!completion.label.is_empty());
            assert_eq!(completion.kind, Some(CompletionItemKind::KEYWORD));
            assert!(completion.label.starts_with("version="));
        }
    }
}
