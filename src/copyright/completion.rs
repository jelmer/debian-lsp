use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position, Uri};

use super::detection::is_copyright_file;
use super::fields::{get_common_licenses, COPYRIGHT_FIELDS};

/// Get completion items for a given position in a copyright file
pub fn get_completions(uri: &Uri, _position: Position) -> Vec<CompletionItem> {
    if !is_copyright_file(uri) {
        return Vec::new();
    }

    let mut completions = Vec::new();
    completions.extend(get_field_completions());
    completions.extend(get_license_completions());
    completions
}

/// Get completion items for copyright file fields
pub fn get_field_completions() -> Vec<CompletionItem> {
    crate::deb822::completion::get_field_completions(COPYRIGHT_FIELDS)
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
    fn test_get_completions_for_copyright_file() {
        let uri = str::parse("file:///path/to/debian/copyright").unwrap();
        let position = Position::new(0, 0);

        let completions = get_completions(&uri, position);
        assert!(!completions.is_empty());

        // Should have both field and license completions
        let field_count = completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::FIELD))
            .count();
        let license_count = completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::VALUE))
            .count();

        assert!(field_count > 0);
        // Only check for licenses if /usr/share/common-licenses exists
        // On macOS/Windows this directory won't exist
        if std::path::Path::new("/usr/share/common-licenses").exists() {
            assert!(license_count > 0);
        }
    }

    #[test]
    fn test_get_completions_for_non_copyright_file() {
        let uri = str::parse("file:///path/to/other.txt").unwrap();
        let position = Position::new(0, 0);

        let completions = get_completions(&uri, position);
        assert!(completions.is_empty());
    }

    #[test]
    fn test_field_completions() {
        let completions = get_field_completions();

        assert!(!completions.is_empty());

        // Check that all completions have required properties
        for completion in &completions {
            assert!(!completion.label.is_empty());
            assert_eq!(completion.kind, Some(CompletionItemKind::FIELD));
            assert!(completion.detail.is_some());
            assert!(completion.documentation.is_some());
            assert!(completion.insert_text.is_some());
            assert!(completion.insert_text.as_ref().unwrap().ends_with(": "));
        }

        // Check for specific fields
        let labels: Vec<_> = completions.iter().map(|c| &c.label).collect();
        assert!(labels.iter().any(|l| *l == "Format"));
        assert!(labels.iter().any(|l| *l == "Files"));
        assert!(labels.iter().any(|l| *l == "License"));
        assert!(labels.iter().any(|l| *l == "Copyright"));
    }

    #[test]
    fn test_license_completions() {
        let completions = get_license_completions();

        // Only check for licenses if /usr/share/common-licenses exists
        // On macOS/Windows this directory won't exist
        if std::path::Path::new("/usr/share/common-licenses").exists() {
            assert!(!completions.is_empty());

            // Check that all completions have required properties
            for completion in &completions {
                assert!(!completion.label.is_empty());
                assert_eq!(completion.kind, Some(CompletionItemKind::VALUE));
                assert_eq!(completion.detail, Some("License name".to_string()));
            }

            // Check for specific licenses
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
