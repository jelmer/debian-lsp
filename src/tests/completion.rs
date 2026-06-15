use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use super::detection::is_executable;
use crate::architecture::SharedArchitectureList;
use crate::deb822::completion::*;
use crate::package_cache::SharedPackageCache;
use crate::position::Source;
use crate::tests::resolve::tests_directory;

use super::fields::{
    TESTS_DEPENDS_SUBSTITUTION_VALUES, TESTS_FEATURES_VALUES, TESTS_FIELDS,
    TESTS_RESTRICTIONS_VALUES,
};

/// Get completions for a debian/tests/control file at the given cursor position.
///
/// Uses the parsed deb822 document for position-aware completions:
/// if on a field value, returns value completions; otherwise returns
/// field name completions. Relationship field completions (Depends, Architecture)
/// are not included here because they require async access to the package cache;
/// use [`get_async_field_value_completions`] for those.
pub fn get_completions(
    deb822: &deb822_lossless::Deb822,
    src: Source<'_>,
    position: Position,
    source_root: Option<&std::path::Path>,
) -> Vec<CompletionItem> {
    let offset = src.try_position_to_offset(position).unwrap_or_default();
    match get_cursor_context(deb822, src, position) {
        Some(CursorContext::FieldValue {
            field_name,
            value_prefix,
        }) => get_field_value_completions(&field_name, &value_prefix, source_root, deb822, offset),
        Some(CursorContext::FieldKey | CursorContext::StartOfLine) => {
            get_field_completions(TESTS_FIELDS)
        }
        None => vec![],
    }
}

/// Get value completions for specific tests/control file fields (sync only).
///
/// Returns completions for Restrictions, Features, Tests, and Tests-Directory fields.
/// Returns empty for relationship fields (handled async separately)
/// and for unknown fields.
pub fn get_field_value_completions(
    field_name: &str,
    prefix: &str,
    source_root: Option<&std::path::Path>,
    deb822: &deb822_lossless::Deb822,
    offset: text_size::TextSize,
) -> Vec<CompletionItem> {
    if field_name.eq_ignore_ascii_case("Restrictions") {
        get_restrictions_value_completions(prefix)
    } else if field_name.eq_ignore_ascii_case("Features") {
        get_features_value_completions(prefix)
    } else if field_name.eq_ignore_ascii_case("Tests") {
        source_root
            .map(|root| get_tests_value_completions(deb822, root, prefix, offset))
            .unwrap_or_default()
    } else if field_name.eq_ignore_ascii_case("Tests-Directory") {
        source_root
            .map(|root| get_tests_directory_value_completions(root, prefix))
            .unwrap_or_default()
    } else {
        vec![]
    }
}

/// Get completion items for the "Restrictions" tests/control field.
///
/// Handles space-separated multiple restrictions: extracts the current token
/// and filters out already-typed restrictions to avoid duplicates.
pub fn get_restrictions_value_completions(prefix: &str) -> Vec<CompletionItem> {
    // Extract the current token (after the last separator)
    let token = if prefix.ends_with(' ') || prefix.ends_with('\n') {
        String::new()
    } else {
        prefix
            .split(|c: char| c == ' ' || c == '\n')
            .filter(|s| !s.is_empty())
            .last()
            .unwrap_or("")
            .to_ascii_lowercase()
    };

    // Extract already-typed values to avoid duplicates
    let existing: Vec<&str> = prefix
        .split(|c: char| c == ' ' || c == '\n')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    TESTS_RESTRICTIONS_VALUES
        .iter()
        .filter(|(value, _)| {
            (token.is_empty() || value.starts_with(&token)) && !existing.contains(value)
        })
        .map(|&(value, description)| CompletionItem {
            label: value.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some(description.to_string()),
            insert_text: Some(value.to_string()),
            ..Default::default()
        })
        .collect()
}

/// Get completion items for the "Features" tests/control field.
///
/// Handles space-separated multiple features: extracts the current token
/// and filters out already-typed features to avoid duplicates.
pub fn get_features_value_completions(prefix: &str) -> Vec<CompletionItem> {
    // Extract the current token (after the last separator)
    let token = if prefix.ends_with(' ') || prefix.ends_with('\n') {
        String::new()
    } else {
        prefix
            .split(|c: char| c == ' ' || c == '\n')
            .filter(|s| !s.is_empty())
            .last()
            .unwrap_or("")
            .to_ascii_lowercase()
    };

    // Extract already-typed values to avoid duplicates
    let existing: Vec<&str> = prefix
        .split(|c: char| c == ' ' || c == '\n')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    TESTS_FEATURES_VALUES
        .iter()
        .filter(|(value, _)| {
            (token.is_empty() || value.starts_with(&token)) && !existing.contains(value)
        })
        .map(|&(value, description)| CompletionItem {
            label: value.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some(description.to_string()),
            insert_text: Some(value.to_string()),
            ..Default::default()
        })
        .collect()
}

/// Get completion items for the "Tests-Directory" tests/control field.
///
/// Scans the source tree one level at a time based on what the user has
/// already typed — lazy completion that avoids scanning the whole tree.
/// Accepts a single path value only; returns empty once the value is complete.
pub fn get_tests_directory_value_completions(
    source_root: &std::path::Path,
    prefix: &str,
) -> Vec<CompletionItem> {
    let trimmed = prefix.trim();

    // Tests-Directory takes a single value — stop completing once done
    if trimmed.ends_with(' ') || prefix.ends_with(' ') {
        return vec![];
    }

    let scan_dir = if let Some(slash_pos) = trimmed.rfind('/') {
        source_root.join(&trimmed[..slash_pos])
    } else {
        source_root.to_path_buf()
    };

    let Ok(entries) = std::fs::read_dir(&scan_dir) else {
        return vec![];
    };

    // The partial name the user is currently typing after the last '/'
    let last_segment = trimmed
        .rfind('/')
        .map(|i| &trimmed[i + 1..])
        .unwrap_or(trimmed);

    entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let path = e.path();
            let rel = path.strip_prefix(&scan_dir).ok()?;
            let s = rel.to_str()?.to_string();
            if !s.is_empty() && s.starts_with(last_segment) && s != last_segment {
                Some(s)
            } else {
                None
            }
        })
        .map(|d| {
            let label = if let Some(slash_pos) = trimmed.rfind('/') {
                format!("{}/{}", &trimmed[..slash_pos], d)
            } else {
                d
            };
            CompletionItem {
                label,
                kind: Some(CompletionItemKind::FOLDER),
                ..Default::default()
            }
        })
        .collect()
}

/// Get completion items for the "Tests" tests/control field.
///
/// Lists executable files in the tests directory (debian/tests/ by default,
/// or the path specified in Tests-Directory). Handles space-separated multiple
/// test names and filters out already-typed names to avoid duplicates.
pub fn get_tests_value_completions(
    deb822: &deb822_lossless::Deb822,
    source_root: &std::path::Path,
    prefix: &str,
    offset: text_size::TextSize,
) -> Vec<CompletionItem> {
    // Extract the current token (after the last separator)
    let token = if prefix.ends_with(' ') || prefix.ends_with('\n') {
        ""
    } else {
        prefix
            .split(|c: char| c == ' ' || c == '\n')
            .filter(|s| !s.is_empty())
            .last()
            .unwrap_or("")
    };

    // Extract already-typed values to avoid duplicates
    let existing: Vec<&str> = prefix
        .split(|c: char| c == ' ' || c == '\n')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    // Find the current paragraph to get its Tests-Directory
    let current_paragraph = deb822
        .paragraphs()
        .find(|p| p.text_range().contains_inclusive(offset));

    // Use Tests-Directory if specified, otherwise fall back to debian/tests
    let tests_dir = tests_directory(current_paragraph.as_ref(), source_root);

    let Ok(entries) = std::fs::read_dir(&tests_dir) else {
        return vec![];
    };

    entries
        .flatten()
        .filter(|e| e.path().is_file() && is_executable(&e.path()))
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if name.starts_with(token) && !existing.contains(&name.as_str()) {
                Some(name)
            } else {
                None
            }
        })
        .map(|name| CompletionItem {
            label: name,
            kind: Some(CompletionItemKind::VALUE),
            ..Default::default()
        })
        .collect()
}

/// Get async value completions for tests/control fields that need the package cache.
///
/// Returns `Some` with completions for relationship fields (Depends) and
/// Architecture, `None` for other fields.
pub async fn get_async_field_value_completions(
    field_name: &str,
    prefix: &str,
    position: Position,
    package_cache: &SharedPackageCache,
    architecture_list: &SharedArchitectureList,
) -> Option<Vec<CompletionItem>> {
    if field_name.eq_ignore_ascii_case("Depends") {
        // Reuse the relationship completion logic from control, with the
        // addition of the substitution @ tokens specific to tests/control.
        let mut completions = crate::control::relation_completion::get_relationship_completions(
            prefix,
            position,
            package_cache,
            architecture_list,
        )
        .await;
        // Add substitution @ tokens if prefix matches
        completions.extend(get_depends_substitution_completions(prefix));
        Some(completions)
    } else if field_name.eq_ignore_ascii_case("Architecture") {
        // Reuse the architecture completion logic from control
        let mut completions = crate::control::completion::get_architecture_value_completions(
            prefix,
            architecture_list,
        )
        .await;

        // Filter out already-typed architectures to avoid duplicates
        let existing: Vec<&str> = prefix
            .split(|c: char| c == ' ' || c == '\n')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        completions.retain(|c| !existing.contains(&c.label.as_str()));
        Some(completions)
    } else {
        None
    }
}

/// Get completion items for the substitution @ tokens in the Depends field.
///
/// These are specific to debian/tests/control and not available in debian/control.
fn get_depends_substitution_completions(prefix: &str) -> Vec<CompletionItem> {
    // Complete the last token after the last separator
    let current_token = if prefix.ends_with(' ') || prefix.ends_with('\n') {
        ""
    } else {
        prefix
            .split(|c: char| c == ' ' || c == '\n')
            .filter(|s| !s.is_empty())
            .last()
            .unwrap_or("")
    };

    // Extract already-typed substitution variables to avoid duplicates
    let existing: Vec<&str> = prefix
        .split(|c: char| c == ' ' || c == '\n')
        .map(|s| s.trim())
        .filter(|s| s.starts_with('@'))
        .collect();

    TESTS_DEPENDS_SUBSTITUTION_VALUES
        .iter()
        .filter(|(value, _)| value.starts_with(current_token))
        .filter(|(value, _)| !existing.contains(value))
        .map(|&(value, description)| CompletionItem {
            label: value.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some(description.to_string()),
            insert_text: Some(value.to_string()),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::architecture::SharedArchitectureList;
    use crate::package_cache::TestPackageCache;
    use crate::position::LineIndex;
    use std::sync::Arc;
    use tower_lsp_server::ls_types::Position;

    fn parse(text: &str) -> deb822_lossless::Deb822 {
        deb822_lossless::Deb822::parse(text).to_result().unwrap()
    }

    fn test_cache() -> SharedPackageCache {
        TestPackageCache::new_shared(&[
            ("cmake", Some("cross-platform make")),
            ("debhelper-compat", None),
            (
                "dh-python",
                Some("Debian helper tools for packaging Python"),
            ),
            ("libssl-dev", None),
            ("pkg-config", None),
        ])
    }

    fn test_arch_list() -> SharedArchitectureList {
        Arc::new(tokio::sync::RwLock::new(vec![
            "amd64".to_string(),
            "arm64".to_string(),
            "armhf".to_string(),
            "i386".to_string(),
        ]))
    }

    #[test]
    fn test_get_completions_on_field_key() {
        let text = "Tests: smoke\nRestrictions: \n";
        let deb822 = parse(text);
        let idx = LineIndex::new(text);
        let completions =
            get_completions(&deb822, Source::new(text, &idx), Position::new(0, 3), None);
        assert!(completions
            .iter()
            .all(|c| c.kind == Some(CompletionItemKind::FIELD)));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"Tests"));
        assert!(labels.contains(&"Restrictions"));
        assert!(labels.contains(&"Depends"));
    }

    #[test]
    fn test_restrictions_value_completions() {
        let completions = get_restrictions_value_completions("");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"needs-root"));
        assert!(labels.contains(&"superficial"));
        assert!(labels.contains(&"allow-stderr"));
    }

    #[test]
    fn test_restrictions_value_completions_with_prefix() {
        let completions = get_restrictions_value_completions("needs-");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"needs-root"));
        assert!(labels.contains(&"needs-internet"));
        assert!(labels.contains(&"needs-reboot"));
        assert!(labels.contains(&"needs-sudo"));
        assert!(!labels.contains(&"superficial"));
    }

    #[test]
    fn test_restrictions_no_duplicate_after_space() {
        let completions = get_restrictions_value_completions("needs-root ");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(!labels.contains(&"needs-root"));
    }

    #[test]
    fn test_restrictions_no_duplicate_second_token() {
        let completions = get_restrictions_value_completions("needs-root flaky ");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(!labels.contains(&"needs-root"));
        assert!(!labels.contains(&"flaky"));
        assert!(labels.contains(&"superficial"));
    }

    #[test]
    fn test_restrictions_token_filter() {
        let completions = get_restrictions_value_completions("needs-root all");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"allow-stderr"));
        assert!(!labels.contains(&"needs-root"));
        assert!(!labels.contains(&"flaky"));
    }

    #[test]
    fn test_get_field_value_completions_for_restrictions() {
        let completions = get_field_value_completions(
            "Restrictions",
            "super",
            None,
            &parse(""),
            text_size::TextSize::default(),
        );
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["superficial"]);
    }

    #[test]
    fn test_features_value_completions() {
        let completions = get_features_value_completions("");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"test-name"));
    }

    #[test]
    fn test_features_value_completions_with_prefix() {
        let completions = get_features_value_completions("test");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["test-name"]);
    }

    #[test]
    fn test_features_no_duplicate_after_space() {
        let completions = get_features_value_completions("test-name ");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(!labels.contains(&"test-name"));
    }

    #[test]
    fn test_get_field_value_completions_for_features() {
        let completions = get_field_value_completions(
            "Features",
            "test",
            None,
            &parse(""),
            text_size::TextSize::default(),
        );
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["test-name"]);
    }

    #[test]
    fn test_depends_substitution_completions() {
        let completions = get_depends_substitution_completions("");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"@"));
        assert!(labels.contains(&"@builddeps@"));
        assert!(labels.contains(&"@recommends@"));
    }

    #[test]
    fn test_depends_substitution_completions_with_prefix() {
        let completions = get_depends_substitution_completions("@b");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["@builddeps@"]);
    }

    #[test]
    fn test_depends_substitution_no_duplicate_after_space() {
        let completions = get_depends_substitution_completions("@ ");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(!labels.contains(&"@"));
        assert!(labels.contains(&"@builddeps@"));
        assert!(labels.contains(&"@recommends@"));
    }

    #[test]
    fn test_depends_substitution_no_duplicate_two_values() {
        let completions = get_depends_substitution_completions("@ @builddeps@ ");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(!labels.contains(&"@"));
        assert!(!labels.contains(&"@builddeps@"));
        assert!(labels.contains(&"@recommends@"));
    }

    #[test]
    fn test_get_field_value_completions_for_unknown_field() {
        let completions = get_field_value_completions(
            "Classes",
            "desktop",
            None,
            &parse(""),
            text_size::TextSize::default(),
        );
        assert!(completions.is_empty());
    }

    #[tokio::test]
    async fn test_async_field_value_completions_for_depends() {
        let cache = test_cache();
        let completions = get_async_field_value_completions(
            "Depends",
            "cm",
            Position::new(0, 2),
            &cache,
            &test_arch_list(),
        )
        .await
        .expect("Should return completions");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"cmake"));
    }

    #[tokio::test]
    async fn test_async_field_value_completions_for_depends_substitution() {
        let cache = test_cache();
        let completions = get_async_field_value_completions(
            "Depends",
            "@b",
            Position::new(0, 2),
            &cache,
            &test_arch_list(),
        )
        .await
        .expect("Should return completions");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"@builddeps@"));
        assert!(!labels.contains(&"@recommends@"));
    }

    #[tokio::test]
    async fn test_async_field_value_completions_for_non_relationship() {
        let cache = test_cache();
        let completions = get_async_field_value_completions(
            "Classes",
            "desktop",
            Position::new(0, 7),
            &cache,
            &test_arch_list(),
        )
        .await;
        assert!(completions.is_none());
    }

    #[tokio::test]
    async fn test_async_field_value_completions_for_architecture() {
        let cache = test_cache();
        let completions = get_async_field_value_completions(
            "Architecture",
            "arm",
            Position::new(0, 3),
            &cache,
            &test_arch_list(),
        )
        .await
        .expect("Should return completions");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"arm64"));
        assert!(labels.contains(&"armhf"));
    }

    #[tokio::test]
    async fn test_async_field_value_completions_for_architecture_no_duplicate() {
        let cache = test_cache();
        let completions = get_async_field_value_completions(
            "Architecture",
            "amd64 arm",
            Position::new(0, 9),
            &cache,
            &test_arch_list(),
        )
        .await
        .expect("Should return completions");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(!labels.contains(&"amd64"));
        assert!(labels.contains(&"arm64"));
        assert!(labels.contains(&"armhf"));
    }
}
