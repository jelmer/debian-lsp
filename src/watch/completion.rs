use crate::deb822::completion::FieldInfo;
use crate::position::try_position_to_offset;
use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, Documentation, Position, Uri,
};

use super::detection::is_watch_file;
use super::fields::{OptionValueType, WATCH_FIELDS, WATCH_LINEBASED_VERSIONS, WATCH_VERSIONS};

/// Build a `FieldInfo` slice for deb822 (v5) field name completions.
fn deb822_field_infos() -> Vec<FieldInfo> {
    WATCH_FIELDS
        .iter()
        .map(|f| FieldInfo::new(f.deb822_name, f.description))
        .collect()
}

/// Get completion items for a v1-4 (line-based) watch file using the CST.
pub fn get_linebased_completions(
    uri: &Uri,
    wf: &debian_watch::linebased::WatchFile,
    source_text: &str,
    position: Position,
) -> Vec<CompletionItem> {
    if !is_watch_file(uri) {
        return Vec::new();
    }

    let Some(offset) = try_position_to_offset(source_text, position) else {
        return Vec::new();
    };

    // Walk ancestors of the token at the cursor to determine context
    if let Some(token) = wf.syntax().token_at_offset(offset).right_biased() {
        for ancestor in token.parent_ancestors() {
            match ancestor.kind() {
                debian_watch::SyntaxKind::OPTION => {
                    // If cursor is after '=', offer value completions
                    let has_eq_before_cursor = ancestor.children_with_tokens().any(|el| {
                        el.kind() == debian_watch::SyntaxKind::EQUALS
                            && el.text_range().end() <= offset
                    });
                    if has_eq_before_cursor {
                        let key = ancestor.children_with_tokens().find_map(|el| {
                            if el.kind() == debian_watch::SyntaxKind::KEY {
                                Some(el.as_token()?.text().to_string())
                            } else {
                                None
                            }
                        });
                        let prefix = if token.kind() == debian_watch::SyntaxKind::VALUE {
                            token.text()
                        } else {
                            ""
                        };
                        return key
                            .and_then(|k| {
                                WATCH_FIELDS
                                    .iter()
                                    .find(|f| f.linebased_name == Some(k.as_str()))
                            })
                            .map(|f| (f.complete_values)(prefix))
                            .unwrap_or_default();
                    }
                    // Cursor is on the key name
                    let prefix = if token.kind() == debian_watch::SyntaxKind::KEY {
                        token.text()
                    } else {
                        ""
                    };
                    return get_linebased_option_completions_with_prefix(prefix);
                }
                debian_watch::SyntaxKind::OPTS_LIST => {
                    // Cursor is in opts area but not inside an OPTION node
                    return get_linebased_option_completions_with_prefix("");
                }
                debian_watch::SyntaxKind::VERSION => {
                    // If cursor is after '=', offer version number completions
                    let has_eq_before_cursor = ancestor.children_with_tokens().any(|el| {
                        el.kind() == debian_watch::SyntaxKind::EQUALS
                            && el.text_range().end() <= offset
                    });
                    if has_eq_before_cursor {
                        return get_linebased_version_value_completions();
                    }
                }
                _ => {}
            }
        }
    }

    // Default: offer version and option completions
    let mut completions = Vec::new();
    completions.extend(get_linebased_option_completions());
    completions.extend(get_linebased_version_completions());
    completions
}

/// Get option name completions filtered by prefix for v1-4 watch files.
fn get_linebased_option_completions_with_prefix(prefix: &str) -> Vec<CompletionItem> {
    let normalized = prefix.trim().to_ascii_lowercase();
    WATCH_FIELDS
        .iter()
        .filter_map(|field| {
            let name = field.linebased_name?;
            if !name.starts_with(&normalized) {
                return None;
            }
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
pub fn get_linebased_option_completions() -> Vec<CompletionItem> {
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

/// Get completion items for watch file `version=N` lines (used in default context).
pub fn get_linebased_version_completions() -> Vec<CompletionItem> {
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

/// Get completion items for version numbers (used when cursor is after `version=`).
fn get_linebased_version_value_completions() -> Vec<CompletionItem> {
    WATCH_LINEBASED_VERSIONS
        .iter()
        .map(|version| CompletionItem {
            label: version.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some(format!("Watch file format version {}", version)),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_linebased(text: &str) -> debian_watch::linebased::WatchFile {
        debian_watch::linebased::parse_watch_file(text).tree()
    }

    #[test]
    fn test_get_linebased_completions_for_watch_file() {
        let uri: Uri = str::parse("file:///path/to/debian/watch").unwrap();
        let text = "version=4\n";
        let wf = parse_linebased(text);

        let completions = get_linebased_completions(&uri, &wf, text, Position::new(0, 0));
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
    fn test_get_linebased_completions_for_non_watch_file() {
        let uri: Uri = str::parse("file:///path/to/other.txt").unwrap();
        let text = "version=4\n";
        let wf = parse_linebased(text);

        let completions = get_linebased_completions(&uri, &wf, text, Position::new(0, 0));
        assert!(completions.is_empty());
    }

    #[test]
    fn test_get_linebased_completions_in_opts_option_name() {
        let uri: Uri = str::parse("file:///path/to/debian/watch").unwrap();
        let text = "version=4\nopts=\"mode=git,\" https://example.com\n";
        let wf = parse_linebased(text);

        // Cursor right after the comma — should offer option names
        let completions = get_linebased_completions(&uri, &wf, text, Position::new(1, 14));
        assert!(!completions.is_empty());
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"pgpmode"));
    }

    #[test]
    fn test_get_linebased_completions_in_opts_option_value() {
        let uri: Uri = str::parse("file:///path/to/debian/watch").unwrap();
        let text = "version=4\nopts=\"mode=git\" https://example.com\n";
        let wf = parse_linebased(text);

        // Cursor on the value "git" — should offer mode values
        let completions = get_linebased_completions(&uri, &wf, text, Position::new(1, 13));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"git"));
        assert!(labels.contains(&"lwp"));
        assert!(labels.contains(&"svn"));
    }

    #[test]
    fn test_get_linebased_completions_version_value() {
        let uri: Uri = str::parse("file:///path/to/debian/watch").unwrap();
        let text = "version=4\n";
        let wf = parse_linebased(text);

        // Cursor on the version number after '='
        let completions = get_linebased_completions(&uri, &wf, text, Position::new(0, 9));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["1", "2", "3", "4"]);
        assert!(completions
            .iter()
            .all(|c| c.kind == Some(CompletionItemKind::VALUE)));
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
    fn test_get_completions_deb822_on_template_value() {
        let text = "Version: 5\n\nTemplate: \n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = get_completions_deb822(&deb822, text, Position::new(2, 10));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(
            labels,
            vec!["github", "gitlab", "pypi", "npmregistry", "metacpan"]
        );
    }

    #[test]
    fn test_get_completions_deb822_on_template_value_with_prefix() {
        let text = "Version: 5\n\nTemplate: g\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = get_completions_deb822(&deb822, text, Position::new(2, 11));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels, vec!["github", "gitlab"]);
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
        let completions = get_linebased_option_completions();

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
        let completions = get_linebased_option_completions();
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        // v5-only fields should not appear as line-based options
        assert!(!labels.contains(&"Source"));
        assert!(!labels.contains(&"Matching-Pattern"));
        assert!(!labels.contains(&"Version"));
    }

    #[test]
    fn test_version_completions() {
        let completions = get_linebased_version_completions();

        assert_eq!(completions.len(), WATCH_VERSIONS.len());

        for completion in &completions {
            assert!(!completion.label.is_empty());
            assert_eq!(completion.kind, Some(CompletionItemKind::KEYWORD));
            assert!(completion.label.starts_with("version="));
        }
    }
}
