use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, Documentation, Position, Uri,
};

use super::detection::is_changelog_file;
use super::fields::{get_debian_distributions, URGENCY_LEVELS};

/// Get completion items for a given position in a changelog file
pub fn get_completions(uri: &Uri, _position: Position) -> Vec<CompletionItem> {
    if !is_changelog_file(uri) {
        return Vec::new();
    }

    let mut completions = Vec::new();
    completions.extend(get_distribution_completions());
    completions.extend(get_urgency_completions());
    completions
}

/// Get completion items for Debian distributions
pub fn get_distribution_completions() -> Vec<CompletionItem> {
    get_debian_distributions()
        .iter()
        .map(|dist| CompletionItem {
            label: dist.clone(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Debian distribution".to_string()),
            documentation: Some(Documentation::String(format!(
                "Target distribution: {}",
                dist
            ))),
            ..Default::default()
        })
        .collect()
}

/// Get completion items for urgency levels
pub fn get_urgency_completions() -> Vec<CompletionItem> {
    URGENCY_LEVELS
        .iter()
        .map(|level| CompletionItem {
            label: level.name.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Urgency level".to_string()),
            documentation: Some(Documentation::String(level.description.to_string())),
            insert_text: Some(format!("urgency={}", level.name)),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_completions_for_changelog_file() {
        let uri = str::parse("file:///path/to/debian/changelog").unwrap();
        let position = Position::new(0, 0);

        let completions = get_completions(&uri, position);
        assert!(!completions.is_empty());

        // Should have both distribution and urgency completions
        let value_count = completions
            .iter()
            .filter(|c| c.kind == Some(CompletionItemKind::VALUE))
            .count();

        assert!(value_count > 0);
    }

    #[test]
    fn test_get_completions_for_non_changelog_file() {
        let uri = str::parse("file:///path/to/other.txt").unwrap();
        let position = Position::new(0, 0);

        let completions = get_completions(&uri, position);
        assert!(completions.is_empty());
    }

    #[test]
    fn test_distribution_completions() {
        let completions = get_distribution_completions();

        assert!(!completions.is_empty());

        // Check that all completions have required properties
        for completion in &completions {
            assert!(!completion.label.is_empty());
            assert_eq!(completion.kind, Some(CompletionItemKind::VALUE));
            assert!(completion.detail.is_some());
            assert!(completion.documentation.is_some());
        }

        // Check for specific distributions
        let labels: Vec<_> = completions.iter().map(|c| &c.label).collect();
        assert!(labels.iter().any(|l| *l == "unstable"));
        assert!(labels.iter().any(|l| *l == "stable"));
        assert!(labels.iter().any(|l| *l == "UNRELEASED"));
    }

    #[test]
    fn test_urgency_completions() {
        let completions = get_urgency_completions();

        assert!(!completions.is_empty());
        assert_eq!(completions.len(), 5);

        // Check that all completions have required properties
        for completion in &completions {
            assert!(!completion.label.is_empty());
            assert_eq!(completion.kind, Some(CompletionItemKind::VALUE));
            assert!(completion.detail.is_some());
            assert!(completion.documentation.is_some());
            assert!(completion.insert_text.is_some());
            assert!(completion
                .insert_text
                .as_ref()
                .unwrap()
                .starts_with("urgency="));
        }

        // Check for specific urgency levels
        let labels: Vec<_> = completions.iter().map(|c| &c.label).collect();
        assert!(labels.iter().any(|l| *l == "low"));
        assert!(labels.iter().any(|l| *l == "medium"));
        assert!(labels.iter().any(|l| *l == "high"));
        assert!(labels.iter().any(|l| *l == "critical"));
        assert!(labels.iter().any(|l| *l == "emergency"));
    }
}
