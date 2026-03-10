use crate::deb822::completion::FieldInfo;
use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, Documentation, Position, Uri,
};

use super::detection::is_watch_file;
use super::fields::{OptionValueType, WATCH_FIELDS, WATCH_VERSIONS};

/// Build a `FieldInfo` slice for deb822 (v5) field name completions.
fn deb822_field_infos() -> Vec<FieldInfo> {
    WATCH_FIELDS
        .iter()
        .map(|f| FieldInfo::new(f.deb822_name, f.description))
        .collect()
}

/// Get completion items for a v1-4 (line-based) watch file.
pub fn get_completions(uri: &Uri, _position: Position) -> Vec<CompletionItem> {
    if !is_watch_file(uri) {
        return Vec::new();
    }

    let mut completions = Vec::new();
    completions.extend(get_option_completions());
    completions.extend(get_version_completions());
    completions
}

/// Get completion items for a v5 (deb822) watch file, using position-aware completions.
pub fn get_completions_deb822(
    deb822: &deb822_lossless::Deb822,
    source_text: &str,
    position: Position,
) -> Vec<CompletionItem> {
    let field_infos = deb822_field_infos();
    crate::deb822::completion::get_completions(
        deb822,
        source_text,
        position,
        &field_infos,
        |field_name, prefix| {
            let lower = field_name.to_lowercase();
            WATCH_FIELDS
                .iter()
                .find(|f| f.deb822_name.to_lowercase() == lower)
                .map(|f| (f.complete_values)(prefix))
                .unwrap_or_default()
        },
    )
}

/// Get completion items for v1-4 watch file options (line-based format).
pub fn get_option_completions() -> Vec<CompletionItem> {
    WATCH_FIELDS
        .iter()
        .filter_map(|field| {
            let name = field.linebased_name?;
            let insert_text = match field.value_type {
                OptionValueType::Boolean => name.to_string(),
                OptionValueType::String | OptionValueType::Enum(_) => {
                    format!("{}=", name)
                }
            };

            Some(CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::PROPERTY),
                detail: Some(field.description.to_string()),
                documentation: Some(Documentation::String(field.description.to_string())),
                insert_text: Some(insert_text),
                ..Default::default()
            })
        })
        .collect()
}

/// Get completion items for option values (for enum-type options).
pub fn get_option_value_completions(option_name: &str) -> Vec<CompletionItem> {
    WATCH_FIELDS
        .iter()
        .find(|f| f.linebased_name == Some(option_name))
        .and_then(|field| {
            if let OptionValueType::Enum(values) = field.value_type {
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
    fn test_get_completions_deb822_on_field_key() {
        let text = "Version: 5\n\nSource: https://example.com\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = get_completions_deb822(&deb822, text, Position::new(2, 3));

        let field_count = completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::FIELD))
            .count();
        assert!(field_count > 0);

        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"Source"));
        assert!(labels.contains(&"Matching-Pattern"));
        assert!(labels.contains(&"Version"));
    }

    #[test]
    fn test_get_completions_deb822_on_string_value() {
        let text = "Version: 5\n\nSource: https://example.com\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        // Source is a string field, no value completions
        let completions = get_completions_deb822(&deb822, text, Position::new(2, 15));
        assert!(completions.is_empty());
    }

    #[test]
    fn test_get_completions_deb822_on_boolean_value() {
        let text = "Version: 5\n\nBare: \n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = get_completions_deb822(&deb822, text, Position::new(2, 6));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels, vec!["yes", "no"]);
    }

    #[test]
    fn test_get_completions_deb822_on_enum_value() {
        let text = "Version: 5\n\nMode: \n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = get_completions_deb822(&deb822, text, Position::new(2, 6));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels, vec!["lwp", "git", "svn"]);
    }

    #[test]
    fn test_get_completions_deb822_on_enum_value_with_prefix() {
        let text = "Version: 5\n\nMode: g\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = get_completions_deb822(&deb822, text, Position::new(2, 7));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels, vec!["git"]);
    }

    #[test]
    fn test_get_completions_deb822_on_empty() {
        let text = "";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = get_completions_deb822(&deb822, text, Position::new(0, 0));

        let field_count = completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::FIELD))
            .count();
        assert!(field_count > 0);
    }

    #[test]
    fn test_option_completions() {
        let completions = get_option_completions();

        assert!(!completions.is_empty());

        for completion in &completions {
            assert!(!completion.label.is_empty());
            assert_eq!(completion.kind, Some(CompletionItemKind::PROPERTY));
            assert!(completion.detail.is_some());
            assert!(completion.documentation.is_some());
            assert!(completion.insert_text.is_some());
        }

        let labels: Vec<_> = completions.iter().map(|c| &c.label).collect();
        assert!(labels.iter().any(|l| *l == "mode"));
        assert!(labels.iter().any(|l| *l == "pgpmode"));
        assert!(labels.iter().any(|l| *l == "uversionmangle"));
    }

    #[test]
    fn test_option_completions_exclude_v5_only() {
        let completions = get_option_completions();
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        // v5-only fields should not appear as line-based options
        assert!(!labels.contains(&"Source"));
        assert!(!labels.contains(&"Matching-Pattern"));
        assert!(!labels.contains(&"Version"));
    }

    #[test]
    fn test_option_value_completions() {
        let mode_values = get_option_value_completions("mode");
        assert!(!mode_values.is_empty());
        let mode_labels: Vec<_> = mode_values.iter().map(|c| &c.label).collect();
        assert!(mode_labels.contains(&&"lwp".to_string()));
        assert!(mode_labels.contains(&&"git".to_string()));
        assert!(mode_labels.contains(&&"svn".to_string()));

        let string_values = get_option_value_completions("uversionmangle");
        assert!(string_values.is_empty());

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
