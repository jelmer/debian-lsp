use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use super::fields::{get_common_licenses, COPYRIGHT_FIELDS};

/// Get completions for a copyright file at the given cursor position.
///
/// Uses position-aware completions: if on a field value, returns value
/// completions for the current field; otherwise returns field name completions.
pub fn get_completions(
    deb822: &deb822_lossless::Deb822,
    source_text: &str,
    position: Position,
) -> Vec<CompletionItem> {
    crate::deb822::completion::get_completions(
        deb822,
        source_text,
        position,
        COPYRIGHT_FIELDS,
        get_field_value_completions,
    )
}

/// The standard DEP-5 format URL.
const DEP5_FORMAT_URL: &str = "https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/";

/// Get value completions for a specific copyright field.
fn get_field_value_completions(field_name: &str, prefix: &str) -> Vec<CompletionItem> {
    let prefix = prefix.trim();
    match field_name.to_lowercase().as_str() {
        "format" => get_format_completions(prefix),
        "license" => get_license_completions(prefix),
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

/// Get completion items for common license names, filtered by prefix.
fn get_license_completions(prefix: &str) -> Vec<CompletionItem> {
    let lower_prefix = prefix.to_lowercase();
    get_common_licenses()
        .iter()
        .filter(|license| license.to_lowercase().starts_with(&lower_prefix))
        .map(|license| CompletionItem {
            label: license.clone(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("License name".to_string()),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_completions_returns_fields() {
        let text = "Format: https://example.com\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        // Cursor on field key → field completions only (no license names mixed in)
        let completions = get_completions(&deb822, text, Position::new(0, 3));

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
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let completions = get_completions(&deb822, text, Position::new(0, 0));

        for completion in &completions {
            assert_eq!(completion.kind, Some(CompletionItemKind::FIELD));
            assert!(!completion.label.is_empty());
            assert!(completion.detail.is_some());
            assert!(completion.documentation.is_some());
            assert!(completion.insert_text.as_ref().unwrap().ends_with(": "));
        }
    }

    #[test]
    fn test_license_value_completions() {
        // Only test if /usr/share/common-licenses exists
        if !std::path::Path::new("/usr/share/common-licenses").exists() {
            return;
        }

        let text = "License: \n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        // Cursor on License field value → license name completions
        let completions = get_completions(&deb822, text, Position::new(0, 9));
        assert!(!completions.is_empty());

        for completion in &completions {
            assert_eq!(completion.kind, Some(CompletionItemKind::VALUE));
            assert_eq!(completion.detail, Some("License name".to_string()));
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
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = get_completions(&deb822, text, Position::new(0, 12));

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
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = get_completions(&deb822, text, Position::new(0, 8));
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].label, DEP5_FORMAT_URL);
        assert_eq!(completions[0].kind, Some(CompletionItemKind::VALUE));
    }

    #[test]
    fn test_format_value_completions_with_non_matching_prefix() {
        let text = "Format: something-else\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = get_completions(&deb822, text, Position::new(0, 22));
        assert!(completions.is_empty());
    }

    #[test]
    fn test_unknown_field_value_returns_empty() {
        let text = "Comment: \n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = get_completions(&deb822, text, Position::new(0, 9));
        assert!(completions.is_empty());
    }
}
