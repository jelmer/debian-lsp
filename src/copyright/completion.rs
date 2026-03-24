use std::collections::HashSet;

use debian_copyright::LicenseExpr;
use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, InsertTextFormat, Position};

use super::fields::{get_common_licenses, COPYRIGHT_FIELDS};

/// Get completions for a copyright file at the given cursor position.
///
/// Uses position-aware completions: if on a field value, returns value
/// completions for the current field; otherwise returns field name completions.
pub fn get_completions(
    parsed: &debian_copyright::lossless::Parse,
    source_text: &str,
    position: Position,
) -> Vec<CompletionItem> {
    let copyright = parsed.tree();
    let deb822 = copyright.as_deb822();

    // Collect all license names used in the file for completion
    let mut file_licenses: HashSet<String> = HashSet::new();
    for files_para in copyright.iter_files() {
        if let Some(license) = files_para.license() {
            if let Some(expr) = license.expr() {
                for name in expr.license_names() {
                    file_licenses.insert(name.to_string());
                }
            }
        }
    }
    for license_para in copyright.iter_licenses() {
        if let Some(name) = license_para.name() {
            for n in LicenseExpr::parse(&name).license_names() {
                file_licenses.insert(n.to_string());
            }
        }
    }
    let mut completions = crate::deb822::completion::get_completions(
        &deb822,
        source_text,
        position,
        COPYRIGHT_FIELDS,
        |field_name, prefix| get_field_value_completions(field_name, prefix, &file_licenses),
    );

    // Offer snippet completions at positions where new paragraphs can be started
    let context = crate::deb822::completion::get_cursor_context(deb822, source_text, position);
    match context {
        Some(crate::deb822::completion::CursorContext::StartOfLine) => {
            if source_text.trim().is_empty() {
                completions.extend(get_snippet_completions());
            } else {
                completions.extend(get_paragraph_snippet_completions());
            }
        }
        Some(crate::deb822::completion::CursorContext::FieldKey)
            if source_text.trim().is_empty() =>
        {
            completions.extend(get_snippet_completions());
        }
        _ => {}
    }

    completions
}

/// The standard DEP-5 format URL.
const DEP5_FORMAT_URL: &str = "https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/";

/// Get snippet completions for scaffolding a new copyright file from scratch.
fn get_snippet_completions() -> Vec<CompletionItem> {
    let mut snippets = vec![
        CompletionItem {
            label: "DEP-5 copyright file".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some("Scaffold a complete DEP-5 copyright file".to_string()),
            insert_text: Some(format!(
                "Format: {}\n\
                 Upstream-Name: ${{1:package}}\n\
                 Upstream-Contact: ${{2:name <email>}}\n\
                 Source: ${{3:url}}\n\
                 \n\
                 Files: *\n\
                 Copyright: ${{4:year}} ${{5:author}}\n\
                 License: ${{6:license}}\n\
                 \n\
                 Files: debian/*\n\
                 Copyright: ${{7:year}} ${{8:maintainer}}\n\
                 License: ${{9:license}}\n\
                 \n\
                 License: ${{6:license}}\n\
                 ${{10:License text.}}\n",
                DEP5_FORMAT_URL,
            )),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some("0".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "DEP-5 header".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some("Scaffold the DEP-5 header paragraph".to_string()),
            insert_text: Some(format!(
                "Format: {}\n\
                 Upstream-Name: ${{1:package}}\n\
                 Upstream-Contact: ${{2:name <email>}}\n\
                 Source: ${{3:url}}\n",
                DEP5_FORMAT_URL,
            )),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some("1".to_string()),
            ..Default::default()
        },
    ];
    snippets.extend(get_paragraph_snippet_completions());
    snippets
}

/// Get snippet completions for adding new paragraphs to an existing copyright file.
fn get_paragraph_snippet_completions() -> Vec<CompletionItem> {
    vec![
        CompletionItem {
            label: "Files paragraph".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some("Scaffold a Files paragraph".to_string()),
            insert_text: Some(
                "Files: ${1:*}\n\
                 Copyright: ${2:year} ${3:author}\n\
                 License: ${4:license}\n"
                    .to_string(),
            ),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some("2".to_string()),
            ..Default::default()
        },
        CompletionItem {
            label: "License paragraph".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some("Scaffold a standalone License paragraph".to_string()),
            insert_text: Some(
                "License: ${1:license}\n\
                 ${2:License text.}\n"
                    .to_string(),
            ),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some("3".to_string()),
            ..Default::default()
        },
    ]
}

/// Get value completions for a specific copyright field.
fn get_field_value_completions(
    field_name: &str,
    prefix: &str,
    file_licenses: &HashSet<String>,
) -> Vec<CompletionItem> {
    let prefix = prefix.trim_start();
    match field_name.to_lowercase().as_str() {
        "format" => get_format_completions(prefix.trim_end()),
        "license" => get_license_completions(prefix, file_licenses),
        _ => vec![],
    }
}

/// Get completion items for the Format field.
fn get_format_completions(prefix: &str) -> Vec<CompletionItem> {
    if DEP5_FORMAT_URL.starts_with(prefix) {
        vec![CompletionItem {
            label: DEP5_FORMAT_URL.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("DEP-5 copyright format".to_string()),
            ..Default::default()
        }]
    } else {
        vec![]
    }
}

/// Extract the last token from a license expression for prefix matching.
///
/// License expressions look like `GPL-2+ or MIT or A`. The last
/// whitespace-separated token is what the user is currently typing.
/// Returns `(last_token, expects_license)` where `expects_license` is true
/// when the last complete token was "or"/"and" (or the expression is empty),
/// meaning the user should see license name completions.
fn last_expression_token(prefix: &str) -> (&str, bool) {
    let trimmed = prefix.trim_end();
    // If prefix ends with whitespace, the user finished a token and is starting a new one
    let ends_with_space = prefix.len() > trimmed.len();

    if trimmed.is_empty() {
        return ("", true);
    }

    let last_token = trimmed
        .rsplit_once(char::is_whitespace)
        .map_or(trimmed, |(_, t)| t);

    if ends_with_space {
        // The last complete token tells us what to expect next
        let lower = last_token.to_lowercase();
        if lower == "or" || lower == "and" {
            // After "or"/"and", expect a license name
            ("", true)
        } else {
            // After a license name, expect "or"/"and"
            ("", false)
        }
    } else {
        // User is mid-token; check what came before to know if this is a license or keyword
        if let Some((before, _)) = trimmed.rsplit_once(char::is_whitespace) {
            let prev_token = before
                .rsplit_once(char::is_whitespace)
                .map_or(before, |(_, t)| t);
            let prev_lower = prev_token.to_lowercase();
            if prev_lower == "or" || prev_lower == "and" {
                (last_token, true)
            } else {
                (last_token, false)
            }
        } else {
            // First token — always a license name
            (last_token, true)
        }
    }
}

/// Get completion items for license expressions, filtered by the current typing context.
fn get_license_completions(prefix: &str, file_licenses: &HashSet<String>) -> Vec<CompletionItem> {
    let (current_token, expects_license) = last_expression_token(prefix);

    if expects_license {
        let lower_token = current_token.to_lowercase();

        // Merge common licenses and file-local licenses, deduplicating
        let mut seen = HashSet::new();
        let mut items = Vec::new();

        // File-local licenses first (more relevant)
        let mut local: Vec<_> = file_licenses.iter().collect();
        local.sort();
        for name in local {
            if name.to_lowercase().starts_with(&lower_token) && seen.insert(name.to_lowercase()) {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::VALUE),
                    detail: Some("License name (from file)".to_string()),
                    sort_text: Some(format!("0{}", name)),
                    ..Default::default()
                });
            }
        }

        // Common system licenses
        for name in get_common_licenses() {
            if name.to_lowercase().starts_with(&lower_token) && seen.insert(name.to_lowercase()) {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::VALUE),
                    detail: Some("License name".to_string()),
                    sort_text: Some(format!("1{}", name)),
                    ..Default::default()
                });
            }
        }

        items
    } else {
        // After a license name, offer "or" and "and" keywords
        let lower_token = current_token.to_lowercase();
        let mut items = Vec::new();
        for keyword in &["or", "and"] {
            if keyword.starts_with(&lower_token) {
                items.push(CompletionItem {
                    label: keyword.to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    detail: Some("License expression operator".to_string()),
                    ..Default::default()
                });
            }
        }
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use debian_copyright::lossless::Parse;

    fn parse(text: &str) -> Parse {
        Parse::parse_relaxed(text)
    }

    #[test]
    fn test_get_completions_returns_fields() {
        let text = "Format: https://example.com\n";
        let parsed = parse(text);

        // Cursor on field key -> field completions only (no license names mixed in)
        let completions = get_completions(&parsed, text, Position::new(0, 3));

        assert!(completions
            .iter()
            .all(|c| c.kind == Some(CompletionItemKind::FIELD)));

        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"Format"));
        assert!(labels.contains(&"Files"));
        assert!(labels.contains(&"License"));
        assert!(labels.contains(&"Copyright"));
    }

    #[test]
    fn test_field_completions_have_correct_properties() {
        let text = "";
        let parsed = parse(text);
        let completions = get_completions(&parsed, text, Position::new(0, 0));

        let field_completions: Vec<_> = completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::FIELD))
            .collect();
        assert!(!field_completions.is_empty());
        for completion in &field_completions {
            assert!(!completion.label.is_empty());
            assert!(completion.detail.is_some());
            assert!(completion.documentation.is_some());
            assert!(completion.insert_text.as_ref().unwrap().ends_with(": "));
        }

        let snippet_completions: Vec<_> = completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::SNIPPET))
            .collect();
        assert!(!snippet_completions.is_empty());
        for completion in &snippet_completions {
            assert!(!completion.label.is_empty());
            assert!(completion.detail.is_some());
            assert_eq!(
                completion.insert_text_format,
                Some(InsertTextFormat::SNIPPET)
            );
        }
    }

    #[test]
    fn test_license_value_completions() {
        // Only test if /usr/share/common-licenses exists
        if !std::path::Path::new("/usr/share/common-licenses").exists() {
            return;
        }

        let text = "License: \n";
        let parsed = parse(text);

        // Cursor on License field value -> license name completions
        let completions = get_completions(&parsed, text, Position::new(0, 9));
        assert!(!completions.is_empty());

        for completion in &completions {
            assert_eq!(completion.kind, Some(CompletionItemKind::VALUE));
        }

        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(
            labels
                .iter()
                .any(|l| l.contains("GPL") || l.contains("Apache")),
            "Should contain common licenses, got: {:?}",
            labels
        );
    }

    #[test]
    fn test_license_value_completions_with_prefix() {
        if !std::path::Path::new("/usr/share/common-licenses").exists() {
            return;
        }

        let text = "License: GPL\n";
        let parsed = parse(text);

        let completions = get_completions(&parsed, text, Position::new(0, 12));

        // All results should match the GPL prefix
        for completion in &completions {
            assert!(
                completion.label.to_lowercase().starts_with("gpl"),
                "Expected GPL prefix, got: {}",
                completion.label
            );
        }
    }

    #[test]
    fn test_format_value_completions() {
        let text = "Format: \n";
        let parsed = parse(text);

        let completions = get_completions(&parsed, text, Position::new(0, 8));
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].label, DEP5_FORMAT_URL);
        assert_eq!(completions[0].kind, Some(CompletionItemKind::VALUE));
    }

    #[test]
    fn test_format_value_completions_with_non_matching_prefix() {
        let text = "Format: something-else\n";
        let parsed = parse(text);

        let completions = get_completions(&parsed, text, Position::new(0, 22));
        assert!(completions.is_empty());
    }

    #[test]
    fn test_unknown_field_value_returns_empty() {
        let text = "Comment: \n";
        let parsed = parse(text);

        let completions = get_completions(&parsed, text, Position::new(0, 9));
        assert!(completions.is_empty());
    }

    #[test]
    fn test_license_completions_from_file() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/*
Copyright: 2024 Alice
License: CustomLicense-1.0

Files: lib/*
Copyright: 2024 Bob
License: \n";
        let parsed = parse(text);

        let completions = get_completions(&parsed, text, Position::new(8, 9));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(
            labels.contains(&"CustomLicense-1.0"),
            "Should include license from file, got: {:?}",
            labels
        );
    }

    #[test]
    fn test_license_or_and_completion() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Test
License: MIT \n";
        let parsed = parse(text);

        // Cursor after "MIT " — should offer "or" and "and"
        let completions = get_completions(&parsed, text, Position::new(4, 13));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(
            labels.contains(&"or"),
            "Should offer 'or' after license name, got: {:?}",
            labels
        );
        assert!(
            labels.contains(&"and"),
            "Should offer 'and' after license name, got: {:?}",
            labels
        );
    }

    #[test]
    fn test_license_completion_after_or() {
        if !std::path::Path::new("/usr/share/common-licenses").exists() {
            return;
        }

        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Test
License: MIT or \n";
        let parsed = parse(text);

        // Cursor after "MIT or " — should offer license names
        let completions = get_completions(&parsed, text, Position::new(4, 16));
        assert!(!completions.is_empty());

        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(
            labels.iter().any(|l| *l != "or" && *l != "and"),
            "Should offer license names after 'or', got: {:?}",
            labels
        );
    }

    #[test]
    fn test_license_completion_after_or_with_prefix() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/*
Copyright: 2024 Alice
License: MIT

Files: *
Copyright: 2024 Test
License: GPL-2+ or MI\n";
        let parsed = parse(text);

        // Cursor after "GPL-2+ or MI" — should offer MIT from file
        let completions = get_completions(&parsed, text, Position::new(8, 21));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(
            labels.contains(&"MIT"),
            "Should offer 'MIT' matching prefix 'MI', got: {:?}",
            labels
        );
    }

    #[test]
    fn test_license_expression_with_or_parses_names() {
        // The file_licenses should pick up both GPL-2+ and MIT
        // from the expression "GPL-2+ or MIT"
        // Verify by checking completions on an empty License field
        let text_with_empty = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/*
Copyright: 2024 Alice
License: GPL-2+ or MIT

Files: lib/*
Copyright: 2024 Bob
License: \n";
        let parsed2 = parse(text_with_empty);
        let completions = get_completions(&parsed2, text_with_empty, Position::new(8, 9));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(
            labels.contains(&"GPL-2+"),
            "Should include GPL-2+ from expression, got: {:?}",
            labels
        );
        assert!(
            labels.contains(&"MIT"),
            "Should include MIT from expression, got: {:?}",
            labels
        );
    }

    #[test]
    fn test_last_expression_token() {
        // Empty
        assert_eq!(last_expression_token(""), ("", true));
        assert_eq!(last_expression_token("  "), ("", true));

        // Single license being typed
        assert_eq!(last_expression_token("MI"), ("MI", true));
        assert_eq!(last_expression_token("GPL-2+"), ("GPL-2+", true));

        // After a complete license name (space after)
        assert_eq!(last_expression_token("MIT "), ("", false));

        // Typing "or"/"and" after a license
        assert_eq!(last_expression_token("MIT o"), ("o", false));
        assert_eq!(last_expression_token("MIT or"), ("or", false));

        // After "or " — expecting a license
        assert_eq!(last_expression_token("MIT or "), ("", true));

        // Typing a license after "or"
        assert_eq!(last_expression_token("MIT or G"), ("G", true));
        assert_eq!(last_expression_token("MIT or GPL-2+"), ("GPL-2+", true));

        // After "and "
        assert_eq!(last_expression_token("MIT and "), ("", true));
        assert_eq!(last_expression_token("MIT and A"), ("A", true));

        // After second license
        assert_eq!(last_expression_token("MIT or GPL-2+ "), ("", false));
    }

    #[test]
    fn test_snippet_completions_on_empty_file() {
        let text = "";
        let parsed = parse(text);
        let completions = get_completions(&parsed, text, Position::new(0, 0));

        let mut snippet_labels: Vec<_> = completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::SNIPPET))
            .map(|c| c.label.as_str())
            .collect();
        snippet_labels.sort();

        assert_eq!(
            snippet_labels,
            vec![
                "DEP-5 copyright file",
                "DEP-5 header",
                "Files paragraph",
                "License paragraph",
            ]
        );

        for snippet in completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::SNIPPET))
        {
            assert_eq!(snippet.insert_text_format, Some(InsertTextFormat::SNIPPET));
            assert!(snippet.insert_text.is_some());
        }
    }

    #[test]
    fn test_snippet_completions_on_whitespace_only_file() {
        let text = "  \n\n";
        let parsed = parse(text);
        let completions = get_completions(&parsed, text, Position::new(2, 0));

        let mut snippet_labels: Vec<_> = completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::SNIPPET))
            .map(|c| c.label.as_str())
            .collect();
        snippet_labels.sort();

        assert_eq!(
            snippet_labels,
            vec![
                "DEP-5 copyright file",
                "DEP-5 header",
                "Files paragraph",
                "License paragraph",
            ]
        );
    }

    #[test]
    fn test_paragraph_snippets_on_non_empty_file() {
        let text = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\n";
        let parsed = parse(text);
        // Cursor at start of blank line after header paragraph
        let completions = get_completions(&parsed, text, Position::new(1, 0));

        let mut snippet_labels: Vec<_> = completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::SNIPPET))
            .map(|c| c.label.as_str())
            .collect();
        snippet_labels.sort();

        assert_eq!(snippet_labels, vec!["Files paragraph", "License paragraph"]);
    }

    #[test]
    fn test_no_snippets_on_field_value() {
        let text = "Format: \n";
        let parsed = parse(text);
        let completions = get_completions(&parsed, text, Position::new(0, 8));

        let snippet_count = completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::SNIPPET))
            .count();
        assert_eq!(snippet_count, 0);
    }

    #[test]
    fn test_full_file_snippet_content() {
        let snippets = get_snippet_completions();
        let full = snippets
            .iter()
            .find(|c| c.label == "DEP-5 copyright file")
            .expect("Should have full file snippet");
        let text = full.insert_text.as_ref().unwrap();
        assert_eq!(
            text,
            &format!(
                "Format: {}\n\
                 Upstream-Name: ${{1:package}}\n\
                 Upstream-Contact: ${{2:name <email>}}\n\
                 Source: ${{3:url}}\n\
                 \n\
                 Files: *\n\
                 Copyright: ${{4:year}} ${{5:author}}\n\
                 License: ${{6:license}}\n\
                 \n\
                 Files: debian/*\n\
                 Copyright: ${{7:year}} ${{8:maintainer}}\n\
                 License: ${{9:license}}\n\
                 \n\
                 License: ${{6:license}}\n\
                 ${{10:License text.}}\n",
                DEP5_FORMAT_URL,
            )
        );
    }
}
