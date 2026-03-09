use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use super::fields::{get_common_licenses, COPYRIGHT_FIELDS};

/// Get completions for a copyright file at the given cursor position.
///
/// Uses position-aware completions: if on a field value, returns value
/// completions; otherwise returns field name and license completions.
pub fn get_completions(
    deb822: &deb822_lossless::Deb822,
    source_text: &str,
    position: Position,
) -> Vec<CompletionItem> {
    let mut completions = crate::deb822::completion::get_completions(
        deb822,
        source_text,
        position,
        COPYRIGHT_FIELDS,
        |_field_name, _prefix| None, // No value completions for copyright fields yet
    );
    // When returning field completions, also include license names.
    if completions
        .iter()
        .any(|c| c.kind == Some(CompletionItemKind::FIELD))
    {
        completions.extend(get_license_completions());
    }
    completions
}

/// Get completion items for common license names
pub fn get_license_completions() -> Vec<CompletionItem> {
    get_common_licenses()
        .iter()
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

        // Cursor on field key → field completions
        let completions = get_completions(&deb822, text, Position::new(0, 3));

        let field_count = completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::FIELD))
            .count();
        assert!(field_count > 0);

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

        for completion in completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::FIELD))
        {
            assert!(!completion.label.is_empty());
            assert!(completion.detail.is_some());
            assert!(completion.documentation.is_some());
            assert!(completion.insert_text.as_ref().unwrap().ends_with(": "));
        }
    }

    #[test]
    fn test_license_completions() {
        let completions = get_license_completions();

        // Only check for licenses if /usr/share/common-licenses exists
        if std::path::Path::new("/usr/share/common-licenses").exists() {
            assert!(!completions.is_empty());

            for completion in &completions {
                assert!(!completion.label.is_empty());
                assert_eq!(completion.kind, Some(CompletionItemKind::VALUE));
                assert_eq!(completion.detail, Some("License name".to_string()));
            }

            let labels: Vec<_> = completions.iter().map(|c| &c.label).collect();
            assert!(
                labels
                    .iter()
                    .any(|l| l.contains("GPL") || l.contains("Apache")),
                "Should contain common licenses"
            );
        }
    }
}
