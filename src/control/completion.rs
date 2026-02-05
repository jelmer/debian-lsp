use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, Documentation, Position, Uri,
};

use super::detection::is_control_file;
use super::fields::{COMMON_PACKAGES, CONTROL_FIELDS};

/// Get completion items for a given position in a control file
pub fn get_completions(uri: &Uri, _position: Position) -> Vec<CompletionItem> {
    if !is_control_file(uri) {
        return Vec::new();
    }

    let mut completions = Vec::new();
    completions.extend(get_field_completions());
    completions.extend(get_package_completions());
    completions
}

/// Get completion items for control file fields
pub fn get_field_completions() -> Vec<CompletionItem> {
    CONTROL_FIELDS
        .iter()
        .map(|field| CompletionItem {
            label: field.name.to_string(),
            kind: Some(CompletionItemKind::FIELD),
            detail: Some(field.description.to_string()),
            documentation: Some(Documentation::String(field.description.to_string())),
            insert_text: Some(format!("{}: ", field.name)),
            ..Default::default()
        })
        .collect()
}

/// Get completion items for common package names
pub fn get_package_completions() -> Vec<CompletionItem> {
    COMMON_PACKAGES
        .iter()
        .map(|&package| CompletionItem {
            label: package.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Package name".to_string()),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_completions_for_control_file() {
        let uri = str::parse("file:///path/to/debian/control").unwrap();
        let position = Position::new(0, 0);

        let completions = get_completions(&uri, position);
        assert!(!completions.is_empty());

        // Should have both field and package completions
        let field_count = completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::FIELD))
            .count();
        let package_count = completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::VALUE))
            .count();

        assert!(field_count > 0);
        assert!(package_count > 0);
    }

    #[test]
    fn test_get_completions_for_non_control_file() {
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
        assert!(labels.iter().any(|l| *l == "Source"));
        assert!(labels.iter().any(|l| *l == "Package"));
        assert!(labels.iter().any(|l| *l == "Depends"));
    }

    #[test]
    fn test_package_completions() {
        let completions = get_package_completions();

        assert!(!completions.is_empty());

        // Check that all completions have required properties
        for completion in &completions {
            assert!(!completion.label.is_empty());
            assert_eq!(completion.kind, Some(CompletionItemKind::VALUE));
            assert_eq!(completion.detail, Some("Package name".to_string()));
        }

        // Check for specific packages
        let labels: Vec<_> = completions.iter().map(|c| &c.label).collect();
        assert!(labels.iter().any(|l| *l == "debhelper-compat"));
        assert!(labels.iter().any(|l| *l == "cmake"));
    }
}
