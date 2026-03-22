use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind};

use super::fields::{
    CONTROL_FIELDS, CONTROL_PRIORITY_VALUES, CONTROL_SECTION_AREAS, CONTROL_SECTION_VALUES,
    CONTROL_SPECIAL_SECTION_VALUES, ESSENTIAL_VALUES, MULTI_ARCH_VALUES,
    RULES_REQUIRES_ROOT_VALUES,
};

use super::relation_completion;
use crate::architecture::SharedArchitectureList;
use crate::package_cache::SharedPackageCache;

/// Get completions for a control file at the given cursor position.
///
/// Uses the parsed deb822 document for position-aware completions:
/// if on a field value, returns value completions; otherwise returns
/// field name completions. Relationship field completions are not
/// included here because they require async access to the package cache;
/// use [`get_async_field_value_completions`] for those.
pub fn get_completions(
    deb822: &deb822_lossless::Deb822,
    source_text: &str,
    position: tower_lsp_server::ls_types::Position,
) -> Vec<CompletionItem> {
    crate::deb822::completion::get_completions(
        deb822,
        source_text,
        position,
        CONTROL_FIELDS,
        get_field_value_completions,
    )
}

/// Get value completions for specific control file fields (sync only).
///
/// Returns completions for Section and Priority fields.
/// Returns empty for relationship fields (handled async separately)
/// and for unknown fields.
pub fn get_field_value_completions(field_name: &str, prefix: &str) -> Vec<CompletionItem> {
    if field_name.eq_ignore_ascii_case("Section") {
        get_section_value_completions(prefix)
    } else if field_name.eq_ignore_ascii_case("Priority") {
        get_priority_value_completions(prefix)
    } else if field_name.eq_ignore_ascii_case("Essential") {
        get_essential_value_completions(prefix)
    } else if field_name.eq_ignore_ascii_case("Multi-Arch") {
        get_multiarch_value_completions(prefix)
    } else if field_name.eq_ignore_ascii_case("Rules-Requires-Root") {
        get_rules_requires_root_value_completions(prefix)
    } else {
        vec![]
    }
}

/// Get async value completions for control file fields that need the package cache.
///
/// Returns `Some` with completions for relationship fields, `None` for other fields.
pub async fn get_async_field_value_completions(
    field_name: &str,
    prefix: &str,
    position: tower_lsp_server::ls_types::Position,
    package_cache: &SharedPackageCache,
    architecture_list: &SharedArchitectureList,
) -> Option<Vec<CompletionItem>> {
    if relation_completion::is_relationship_field(field_name) {
        Some(
            relation_completion::get_relationship_completions(
                prefix,
                position,
                package_cache,
                architecture_list,
            )
            .await,
        )
    } else if field_name.eq_ignore_ascii_case("Architecture") {
        Some(get_architecture_value_completions(prefix, architecture_list).await)
    } else {
        None
    }
}

/// Special architecture values that are always available.
const ARCHITECTURE_SPECIAL_VALUES: &[(&str, &str)] = &[
    ("all", "Architecture-independent package"),
    ("any", "Build for any supported architecture"),
];

/// Get completion items for "Architecture" control fields.
///
/// Handles space-separated multiple architectures and the `!` negation prefix.
pub async fn get_architecture_value_completions(
    prefix: &str,
    architecture_list: &SharedArchitectureList,
) -> Vec<CompletionItem> {
    // The prefix is the entire field value up to the cursor.
    // For multiple architectures (space-separated), we only complete the last token.
    let current_token = prefix.rsplit(' ').next().unwrap_or("").trim();

    // Handle negation prefix
    let (negated, arch_prefix) = if let Some(rest) = current_token.strip_prefix('!') {
        (true, rest)
    } else {
        (false, current_token)
    };

    let normalized_prefix = arch_prefix.to_ascii_lowercase();

    let arches = architecture_list.read().await;

    let mut completions = Vec::new();

    // Add special values ("all", "any") — only when not negated
    if !negated {
        for &(value, description) in ARCHITECTURE_SPECIAL_VALUES {
            if value.starts_with(&normalized_prefix) {
                completions.push(CompletionItem {
                    label: value.to_string(),
                    kind: Some(CompletionItemKind::VALUE),
                    detail: Some(description.to_string()),
                    ..Default::default()
                });
            }
        }
    }

    // Add matching architectures, with or without negation prefix
    for arch in arches.iter() {
        if arch.starts_with(&normalized_prefix) {
            let label = if negated {
                format!("!{}", arch)
            } else {
                arch.clone()
            };
            completions.push(CompletionItem {
                label,
                kind: Some(CompletionItemKind::VALUE),
                ..Default::default()
            });
        }
    }

    completions
}

/// Get completion items for "Priority" control field.
pub fn get_priority_value_completions(prefix: &str) -> Vec<CompletionItem> {
    let normalized_prefix = prefix.trim().to_ascii_lowercase();

    CONTROL_PRIORITY_VALUES
        .iter()
        .filter(|(value, _)| value.starts_with(&normalized_prefix))
        .map(|&(value, description)| CompletionItem {
            label: value.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some(description.to_string()),
            insert_text: Some(value.to_string()),
            ..Default::default()
        })
        .collect()
}

/// Get completion items for "Section" control field.
///
/// Includes both `section` and `area/section` forms.
pub fn get_section_value_completions(prefix: &str) -> Vec<CompletionItem> {
    let normalized_prefix = prefix.trim().to_ascii_lowercase();
    let mut completions = Vec::new();

    for &(section, description) in CONTROL_SECTION_VALUES {
        if section.starts_with(&normalized_prefix) {
            completions.push(CompletionItem {
                label: section.to_string(),
                kind: Some(CompletionItemKind::VALUE),
                detail: Some(description.to_string()),
                insert_text: Some(section.to_string()),
                ..Default::default()
            });
        }
    }

    for &area in CONTROL_SECTION_AREAS {
        for &(section, description) in CONTROL_SECTION_VALUES {
            let qualified = format!("{}/{}", area, section);
            if qualified.starts_with(&normalized_prefix) {
                completions.push(CompletionItem {
                    label: qualified.clone(),
                    kind: Some(CompletionItemKind::VALUE),
                    detail: Some(description.to_string()),
                    insert_text: Some(qualified),
                    ..Default::default()
                });
            }
        }
    }

    for &(special, description) in CONTROL_SPECIAL_SECTION_VALUES {
        if special.starts_with(&normalized_prefix) {
            completions.push(CompletionItem {
                label: special.to_string(),
                kind: Some(CompletionItemKind::VALUE),
                detail: Some(description.to_string()),
                insert_text: Some(special.to_string()),
                ..Default::default()
            });
        }
    }

    completions
}

/// Get completion items for "Essential" control field.
pub fn get_essential_value_completions(prefix: &str) -> Vec<CompletionItem> {
    let normalized_prefix = prefix.trim().to_ascii_lowercase();

    ESSENTIAL_VALUES
        .iter()
        .filter(|(value, _)| value.starts_with(&normalized_prefix))
        .map(|&(value, description)| CompletionItem {
            label: value.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some(description.to_string()),
            insert_text: Some(value.to_string()),
            ..Default::default()
        })
        .collect()
}

/// Get completion items for "Multi-Arch" control fields.
pub fn get_multiarch_value_completions(prefix: &str) -> Vec<CompletionItem> {
    let normalized_prefix = prefix.trim().to_ascii_lowercase();

    MULTI_ARCH_VALUES
        .iter()
        .filter(|(value, _)| value.starts_with(&normalized_prefix))
        .map(|&(value, description)| CompletionItem {
            label: value.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some(description.to_string()),
            insert_text: Some(value.to_string()),
            ..Default::default()
        })
        .collect()
}

/// Get completion items for "Rules-Requires-Root" control field.
pub fn get_rules_requires_root_value_completions(prefix: &str) -> Vec<CompletionItem> {
    let normalized_prefix = prefix.trim().to_ascii_lowercase();

    RULES_REQUIRES_ROOT_VALUES
        .iter()
        .filter(|(value, _)| value.starts_with(&normalized_prefix))
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
    use std::sync::Arc;
    use tower_lsp_server::ls_types::Position;

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
        let text = "Source: test\nSection: py\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = get_completions(&deb822, text, Position::new(1, 3));

        // Should have field completions only
        assert!(completions
            .iter()
            .all(|c| c.kind == Some(CompletionItemKind::FIELD)));
    }

    #[test]
    fn test_get_completions_on_section_value() {
        let text = "Source: test\nSection: py\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = get_completions(&deb822, text, Position::new(1, 11));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(labels.contains(&"python"));
    }

    #[test]
    fn test_get_completions_on_priority_value() {
        let text = "Source: test\nPriority: op\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = get_completions(&deb822, text, Position::new(1, 12));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels, vec!["optional"]);
    }

    #[test]
    fn test_priority_value_completions() {
        let completions = get_priority_value_completions("");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(labels.contains(&"required"));
        assert!(labels.contains(&"important"));
        assert!(labels.contains(&"standard"));
        assert!(labels.contains(&"optional"));
        assert!(labels.contains(&"extra"));
    }

    #[test]
    fn test_priority_value_completions_with_prefix() {
        let completions = get_priority_value_completions("op");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["optional"]);
    }

    #[test]
    fn test_priority_value_completions_with_uppercase_prefix() {
        let completions = get_priority_value_completions("OP");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["optional"]);
    }

    #[test]
    fn test_section_value_completions() {
        let completions = get_section_value_completions("");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(labels.contains(&"admin"));
        assert!(labels.contains(&"python"));
        assert!(labels.contains(&"debian-installer"));
        assert!(labels.contains(&"non-free/python"));
        assert!(!labels.contains(&"non-free/debian-installer"));

        // Check that descriptions are present
        let admin = completions.iter().find(|c| c.label == "admin").unwrap();
        assert_eq!(
            admin.detail.as_deref(),
            Some("System administration utilities")
        );
    }

    #[test]
    fn test_section_value_completions_with_area_prefix() {
        let completions = get_section_value_completions("non-free/");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(labels.contains(&"non-free/python"));
        assert!(!labels.contains(&"python"));
        assert!(!labels.contains(&"non-free/debian-installer"));

        // Area-qualified sections use the same description as the base section
        let nf_python = completions
            .iter()
            .find(|c| c.label == "non-free/python")
            .unwrap();
        assert_eq!(
            nf_python.detail.as_deref(),
            Some("Python programming language")
        );
    }

    #[test]
    fn test_get_field_value_completions_for_section() {
        let completions = get_field_value_completions("Section", "py");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"python"));
    }

    #[test]
    fn test_get_field_value_completions_for_priority() {
        let completions = get_field_value_completions("Priority", "op");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["optional"]);
    }

    #[test]
    fn test_get_field_value_completions_for_essential() {
        let completions = get_field_value_completions("Essential", "y");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["yes"]);
    }

    #[test]
    fn test_get_field_value_completions_for_multiarch() {
        let completions = get_field_value_completions("Multi-Arch", "all");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["allowed"]);
    }

    #[test]
    fn test_get_field_value_completions_for_rules_requires_root() {
        let completions = get_field_value_completions("Rules-Requires-Root", "n");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["no"]);
    }

    #[test]
    fn test_rules_requires_root_value_completions() {
        let completions = get_rules_requires_root_value_completions("");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(labels.contains(&"no"));
        assert!(labels.contains(&"binary-targets"));
    }

    #[test]
    fn test_rules_requires_root_value_completions_with_prefix() {
        let completions = get_rules_requires_root_value_completions("bi");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["binary-targets"]);
    }

    #[test]
    fn test_get_field_value_completions_for_unknown_field() {
        let completions = get_field_value_completions("Homepage", "http");
        assert!(completions.is_empty());
    }

    #[test]
    fn test_get_completions_on_essential_value() {
        let text = "Source: test\nEssential: y\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let completions = get_completions(&deb822, text, Position::new(1, 12));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels, vec!["yes"]);
    }

    #[test]
    fn test_essential_value_completions() {
        let completions = get_essential_value_completions("");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(labels.contains(&"yes"));
        assert!(labels.contains(&"no"));
    }

    #[test]
    fn test_essential_value_completions_with_prefix() {
        let completions = get_essential_value_completions("n");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["no"]);
    }

    #[test]
    fn test_essential_value_completions_with_uppercase_prefix() {
        let completions = get_essential_value_completions("YE");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["yes"]);
    }

    #[test]
    fn test_multiarch_value_completions() {
        let completions = get_multiarch_value_completions("");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(labels.contains(&"allowed"));
        assert!(labels.contains(&"foreign"));
        assert!(labels.contains(&"no"));
        assert!(labels.contains(&"same"));
    }

    #[test]
    fn test_multiarch_value_completions_with_prefix() {
        let completions = get_multiarch_value_completions("all");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["allowed"]);
    }

    #[test]
    fn test_multiarch_value_completions_with_uppercase_prefix() {
        let completions = get_multiarch_value_completions("ALL");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["allowed"]);
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
        assert_eq!(labels, vec!["cmake"]);
    }

    #[tokio::test]
    async fn test_async_field_value_completions_for_build_depends() {
        let cache = test_cache();
        let completions = get_async_field_value_completions(
            "Build-Depends",
            "",
            Position::new(0, 0),
            &cache,
            &test_arch_list(),
        )
        .await
        .expect("Should return completions");
        assert!(!completions.is_empty());
    }

    #[tokio::test]
    async fn test_async_field_value_completions_for_non_relationship() {
        let cache = test_cache();
        let completions = get_async_field_value_completions(
            "Homepage",
            "http",
            Position::new(0, 4),
            &cache,
            &test_arch_list(),
        )
        .await;
        assert!(completions.is_none());
    }

    /// End-to-end test: get_cursor_context → get_async_field_value_completions
    /// for a single-line Build-Depends field with cursor after ": ".
    #[tokio::test]
    async fn test_end_to_end_build_depends_empty_value() {
        let text = "Build-Depends: \n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let ctx = crate::deb822::completion::get_cursor_context(
            &deb822,
            text,
            tower_lsp_server::ls_types::Position::new(0, 15),
        )
        .expect("Should have context");

        match ctx {
            crate::deb822::completion::CursorContext::FieldValue {
                field_name,
                value_prefix,
            } => {
                assert_eq!(field_name, "Build-Depends");
                assert_eq!(value_prefix, "");
                let cache = test_cache();
                let completions = get_async_field_value_completions(
                    &field_name,
                    &value_prefix,
                    tower_lsp_server::ls_types::Position::new(0, 15),
                    &cache,
                    &test_arch_list(),
                )
                .await
                .expect("Should return completions for relationship field");
                assert!(!completions.is_empty(), "Should have package completions");
            }
            other => panic!("Expected FieldValue, got {:?}", other),
        }
    }

    /// End-to-end test: cursor in middle of Build-Depends value.
    #[tokio::test]
    async fn test_end_to_end_build_depends_partial_name() {
        let text = "Build-Depends: dh\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let ctx = crate::deb822::completion::get_cursor_context(
            &deb822,
            text,
            tower_lsp_server::ls_types::Position::new(0, 17),
        )
        .expect("Should have context");

        match ctx {
            crate::deb822::completion::CursorContext::FieldValue {
                field_name,
                value_prefix,
            } => {
                assert_eq!(field_name, "Build-Depends");
                assert_eq!(value_prefix, "dh");
                let cache = test_cache();
                let completions = get_async_field_value_completions(
                    &field_name,
                    &value_prefix,
                    tower_lsp_server::ls_types::Position::new(0, 17),
                    &cache,
                    &test_arch_list(),
                )
                .await
                .expect("Should return completions for relationship field");
                let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
                assert_eq!(labels, vec!["dh-python"]);
            }
            other => panic!("Expected FieldValue, got {:?}", other),
        }
    }

    /// End-to-end test: substvar completion after comma should not eat the comma.
    #[tokio::test]
    async fn test_end_to_end_substvar_after_comma() {
        let text = "Depends: gpg,${misc:\n";
        let deb822 = deb822_lossless::Deb822::parse(text).tree();
        let position = Position::new(0, 20);
        let ctx = crate::deb822::completion::get_cursor_context(&deb822, text, position)
            .expect("Should have context");

        match ctx {
            crate::deb822::completion::CursorContext::FieldValue {
                field_name,
                value_prefix,
            } => {
                assert_eq!(field_name, "Depends");
                assert_eq!(value_prefix, "gpg,${misc:");
                let cache = test_cache();
                let completions = get_async_field_value_completions(
                    &field_name,
                    &value_prefix,
                    position,
                    &cache,
                    &test_arch_list(),
                )
                .await
                .expect("Should return completions");
                let misc_depends = completions
                    .iter()
                    .find(|c| c.label == "${misc:Depends}")
                    .expect("Should have ${misc:Depends} completion");
                // The text_edit range must start at the "$" (col 13), NOT at the comma (col 12)
                let edit = match &misc_depends.text_edit {
                    Some(tower_lsp_server::ls_types::CompletionTextEdit::Edit(e)) => e,
                    _ => panic!("Expected TextEdit"),
                };
                assert_eq!(edit.range.start, Position::new(0, 13));
                assert_eq!(edit.range.end, position);
                assert_eq!(edit.new_text, "${misc:Depends}");
            }
            other => panic!("Expected FieldValue, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_architecture_value_completions_empty_prefix() {
        let completions = get_architecture_value_completions("", &test_arch_list()).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(labels.contains(&"all"));
        assert!(labels.contains(&"any"));
        assert!(labels.contains(&"amd64"));
        assert!(labels.contains(&"arm64"));
        assert!(labels.contains(&"armhf"));
        assert!(labels.contains(&"i386"));
    }

    #[tokio::test]
    async fn test_architecture_value_completions_with_prefix() {
        let completions = get_architecture_value_completions("arm", &test_arch_list()).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels.len(), 2);
        assert!(labels.contains(&"arm64"));
        assert!(labels.contains(&"armhf"));
    }

    #[tokio::test]
    async fn test_architecture_value_completions_uppercase_prefix() {
        let completions = get_architecture_value_completions("ARM", &test_arch_list()).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels.len(), 2);
        assert!(labels.contains(&"arm64"));
        assert!(labels.contains(&"armhf"));
    }

    #[tokio::test]
    async fn test_architecture_value_completions_special_values() {
        let completions = get_architecture_value_completions("a", &test_arch_list()).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(labels.contains(&"all"));
        assert!(labels.contains(&"any"));
        assert!(labels.contains(&"amd64"));
        assert!(labels.contains(&"arm64"));
        assert!(labels.contains(&"armhf"));
    }

    #[tokio::test]
    async fn test_architecture_value_completions_special_all() {
        let completions = get_architecture_value_completions("al", &test_arch_list()).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels, vec!["all"]);
        assert!(completions[0].detail.is_some());
    }

    #[tokio::test]
    async fn test_architecture_value_completions_negated() {
        let completions = get_architecture_value_completions("!arm", &test_arch_list()).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels.len(), 2);
        assert!(labels.contains(&"!arm64"));
        assert!(labels.contains(&"!armhf"));
    }

    #[tokio::test]
    async fn test_architecture_value_completions_negated_no_special() {
        let completions = get_architecture_value_completions("!", &test_arch_list()).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        // Negation should not include "all" or "any"
        assert!(!labels.contains(&"!all"));
        assert!(!labels.contains(&"!any"));
        assert!(labels.contains(&"!amd64"));
    }

    #[tokio::test]
    async fn test_architecture_value_completions_multiple_arches() {
        // When user has typed "amd64 arm", we should complete the last token "arm"
        let completions = get_architecture_value_completions("amd64 arm", &test_arch_list()).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels.len(), 2);
        assert!(labels.contains(&"arm64"));
        assert!(labels.contains(&"armhf"));
    }

    #[tokio::test]
    async fn test_architecture_value_completions_multiple_with_negation() {
        let completions = get_architecture_value_completions("any !i", &test_arch_list()).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels, vec!["!i386"]);
    }
}
