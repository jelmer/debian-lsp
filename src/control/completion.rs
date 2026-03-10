use debian_control::lossless::relations::Relations;
use debian_control::relations::SyntaxKind as RelSyntaxKind;
use rowan::NodeOrToken;
use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Documentation};

use super::fields::{
    CONTROL_FIELDS, CONTROL_PRIORITY_VALUES, CONTROL_SECTION_AREAS, CONTROL_SECTION_VALUES,
    CONTROL_SPECIAL_SECTION_VALUES,
};
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
    } else {
        vec![]
    }
}

/// Relationship field names in debian/control.
const RELATIONSHIP_FIELDS: &[&str] = &[
    "Depends",
    "Pre-Depends",
    "Recommends",
    "Suggests",
    "Enhances",
    "Conflicts",
    "Breaks",
    "Provides",
    "Replaces",
    "Build-Depends",
    "Build-Depends-Indep",
    "Build-Depends-Arch",
    "Build-Conflicts",
    "Build-Conflicts-Indep",
    "Build-Conflicts-Arch",
];

/// Returns true if the field name is a relationship field.
fn is_relationship_field(field_name: &str) -> bool {
    RELATIONSHIP_FIELDS
        .iter()
        .any(|f| f.eq_ignore_ascii_case(field_name))
}

/// Version constraint operators in Debian relationships.
const VERSION_OPERATORS: &[(&str, &str)] = &[
    (">=", "Greater than or equal"),
    ("<=", "Less than or equal"),
    ("=", "Exactly equal"),
    (">>", "Strictly greater than"),
    ("<<", "Strictly less than"),
];

/// Where the cursor is within a relationship field value.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RelationCompletionPosition {
    /// At a package name position (start, after comma, or after pipe).
    PackageName(String),
    /// After `(` — expecting a version operator.
    VersionOperator(String),
    /// After a version operator — expecting a version string.
    Version(String),
    /// Inside `[` — expecting an architecture name.
    Architecture(String),
}

/// Determine the completion position within a relationship field value prefix.
///
/// Parses the prefix using the lossless relations parser and walks the
/// concrete syntax tree to determine context at the end of the input.
fn determine_relation_position(prefix: &str) -> RelationCompletionPosition {
    let (relations, _errors) = Relations::parse_relaxed(prefix, true);
    let syntax = relations.syntax();

    // Find the last non-whitespace token in the tree.
    let last_token = last_significant_token(syntax);

    let Some(token) = last_token else {
        return RelationCompletionPosition::PackageName(String::new());
    };

    // Walk up to find what node context we're in.
    let parent_kind = token
        .parent()
        .map(|p| p.kind())
        .unwrap_or(RelSyntaxKind::ROOT);

    // Check if we're inside a VERSION node (i.e. inside parentheses of a version constraint).
    let in_version = token
        .parent_ancestors()
        .any(|n| n.kind() == RelSyntaxKind::VERSION);

    if in_version {
        // Inside a version constraint like "libc6 (>= 2.1" or "libc6 (>" or "libc6 ("
        let version_node = token
            .parent_ancestors()
            .find(|n| n.kind() == RelSyntaxKind::VERSION)
            .unwrap();

        // Check if we have a CONSTRAINT child node with any operator tokens.
        let constraint_text: String = version_node
            .children()
            .find(|n| n.kind() == RelSyntaxKind::CONSTRAINT)
            .map(|c| c.text().to_string())
            .unwrap_or_default();

        if constraint_text.is_empty() {
            // After "(" with no operator yet
            return RelationCompletionPosition::VersionOperator(String::new());
        }

        // Is the last significant token part of the constraint operator?
        // If so, check whether the operator is followed by whitespace in the
        // original input — if yes, the operator is complete and we're in
        // version position; otherwise we're still typing the operator.
        if parent_kind == RelSyntaxKind::CONSTRAINT {
            let has_trailing_ws = token.next_token().is_some_and(|t| {
                matches!(t.kind(), RelSyntaxKind::WHITESPACE | RelSyntaxKind::NEWLINE)
            });
            if has_trailing_ws {
                // Operator is complete, now in version position
                return RelationCompletionPosition::Version(String::new());
            }
            return RelationCompletionPosition::VersionOperator(constraint_text);
        }

        // We have a constraint; we're in version position.
        // Collect IDENT tokens after the constraint as the version prefix.
        let version_prefix: String = version_node
            .children_with_tokens()
            .filter_map(|it| match it {
                NodeOrToken::Token(t)
                    if t.kind() == RelSyntaxKind::IDENT || t.kind() == RelSyntaxKind::COLON =>
                {
                    Some(t.text().to_string())
                }
                _ => None,
            })
            .collect();

        return RelationCompletionPosition::Version(version_prefix);
    }

    // Check if we're inside an ARCHITECTURES node (i.e. inside brackets).
    let in_architectures = token
        .parent_ancestors()
        .any(|n| n.kind() == RelSyntaxKind::ARCHITECTURES);

    if in_architectures {
        match token.kind() {
            RelSyntaxKind::L_BRACKET => {
                return RelationCompletionPosition::Architecture(String::new());
            }
            RelSyntaxKind::IDENT => {
                // If followed by whitespace, the arch name is complete —
                // we're in position for a new architecture.
                let has_trailing_ws = token.next_token().is_some_and(|t| {
                    matches!(t.kind(), RelSyntaxKind::WHITESPACE | RelSyntaxKind::NEWLINE)
                });
                if has_trailing_ws {
                    return RelationCompletionPosition::Architecture(String::new());
                }
                // Check if preceded by "!" (negated arch).
                let negated = token
                    .prev_token()
                    .is_some_and(|t| t.kind() == RelSyntaxKind::NOT);
                let prefix = if negated {
                    format!("!{}", token.text())
                } else {
                    token.text().to_string()
                };
                return RelationCompletionPosition::Architecture(prefix);
            }
            RelSyntaxKind::NOT => {
                // "!" with no arch name yet
                return RelationCompletionPosition::Architecture("!".to_string());
            }
            _ => {
                return RelationCompletionPosition::Architecture(String::new());
            }
        }
    }

    match token.kind() {
        RelSyntaxKind::COMMA | RelSyntaxKind::PIPE => {
            RelationCompletionPosition::PackageName(String::new())
        }
        RelSyntaxKind::IDENT if parent_kind == RelSyntaxKind::RELATION => {
            // Could be a package name being typed
            RelationCompletionPosition::PackageName(token.text().to_string())
        }
        _ => RelationCompletionPosition::PackageName(String::new()),
    }
}

/// Find the last non-whitespace token in a syntax tree.
///
/// We walk forward from `first_token()` rather than using `last_token()`
/// because rowan's `last_token()` returns `None` when the tree ends with
/// an empty node (e.g. an ERROR node from incomplete input).
fn last_significant_token(
    node: &rowan::SyntaxNode<debian_control::lossless::relations::Lang>,
) -> Option<rowan::SyntaxToken<debian_control::lossless::relations::Lang>> {
    let mut result = None;
    let mut tok = node.first_token();
    while let Some(t) = tok {
        if !matches!(t.kind(), RelSyntaxKind::WHITESPACE | RelSyntaxKind::NEWLINE) {
            result = Some(t.clone());
        }
        tok = t.next_token();
    }
    result
}

/// Get completions for a relationship field value.
pub(crate) async fn get_relationship_completions(
    prefix: &str,
    package_cache: &SharedPackageCache,
    architecture_list: &SharedArchitectureList,
) -> Vec<CompletionItem> {
    match determine_relation_position(prefix) {
        RelationCompletionPosition::PackageName(partial) => {
            let cache = package_cache.read().await;
            let packages = cache.get_packages_with_prefix(&partial);
            packages
                .iter()
                .map(|pkg| {
                    let description = cache.get_description(pkg).map(|s| s.to_string());
                    let documentation = description
                        .as_deref()
                        .map(|d| Documentation::String(d.to_string()));
                    CompletionItem {
                        label: pkg.clone(),
                        kind: Some(CompletionItemKind::VALUE),
                        detail: description.or_else(|| Some("Package".to_string())),
                        documentation,
                        ..Default::default()
                    }
                })
                .collect()
        }
        RelationCompletionPosition::VersionOperator(partial) => VERSION_OPERATORS
            .iter()
            .filter(|(op, _)| op.starts_with(&partial))
            .map(|&(op, desc)| CompletionItem {
                label: op.to_string(),
                kind: Some(CompletionItemKind::OPERATOR),
                detail: Some(desc.to_string()),
                insert_text: Some(format!("{} ", op)),
                ..Default::default()
            })
            .collect(),
        RelationCompletionPosition::Version(partial) => {
            get_version_completions(prefix, &partial, package_cache).await
        }
        RelationCompletionPosition::Architecture(partial) => {
            get_architecture_completions(&partial, architecture_list).await
        }
    }
}

/// Get completion items for architecture names.
async fn get_architecture_completions(
    partial: &str,
    architecture_list: &SharedArchitectureList,
) -> Vec<CompletionItem> {
    let negated = partial.starts_with('!');
    let prefix = if negated { &partial[1..] } else { partial };

    let arches = architecture_list.read().await;
    arches
        .iter()
        .filter(|arch| arch.starts_with(prefix))
        .map(|arch| {
            let label = if negated {
                format!("!{}", arch)
            } else {
                arch.clone()
            };
            CompletionItem {
                label,
                kind: Some(CompletionItemKind::VALUE),
                ..Default::default()
            }
        })
        .collect()
}

/// Extract the package name from a relationship prefix when in version position.
///
/// Parses the prefix using the lossless relations parser and returns the
/// name of the last relation (which is the one whose version is being typed).
fn extract_package_for_version(prefix: &str) -> Option<String> {
    let (relations, _errors) = Relations::parse_relaxed(prefix, true);
    // The last entry's last relation is the one with the version being typed.
    let last_entry = relations.entries().last()?;
    let last_relation = last_entry.relations().last()?;
    Some(last_relation.name())
}

/// Get version completions for a package.
async fn get_version_completions(
    prefix: &str,
    partial: &str,
    package_cache: &SharedPackageCache,
) -> Vec<CompletionItem> {
    let Some(package_name) = extract_package_for_version(prefix) else {
        return Vec::new();
    };
    let mut cache = package_cache.write().await;
    let Some(versions) = cache.load_versions(&package_name).await else {
        return Vec::new();
    };
    versions
        .iter()
        .filter(|v| v.version.starts_with(partial))
        .map(|v| {
            let suites = v.suites.join(", ");
            CompletionItem {
                label: v.version.clone(),
                kind: Some(CompletionItemKind::VALUE),
                detail: Some(suites),
                ..Default::default()
            }
        })
        .collect()
}

/// Get async value completions for control file fields that need the package cache.
///
/// Returns `Some` with completions for relationship fields, `None` for other fields.
pub async fn get_async_field_value_completions(
    field_name: &str,
    prefix: &str,
    package_cache: &SharedPackageCache,
    architecture_list: &SharedArchitectureList,
) -> Option<Vec<CompletionItem>> {
    if is_relationship_field(field_name) {
        Some(get_relationship_completions(prefix, package_cache, architecture_list).await)
    } else {
        None
    }
}

/// Get completion items for Debian priority values.
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

/// Get completion items for Debian section values.
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

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_get_field_value_completions_for_unknown_field() {
        let completions = get_field_value_completions("Homepage", "http");
        assert!(completions.is_empty());
    }

    #[tokio::test]
    async fn test_async_field_value_completions_for_depends() {
        let cache = test_cache();
        let completions =
            get_async_field_value_completions("Depends", "cm", &cache, &test_arch_list())
                .await
                .expect("Should return completions");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["cmake"]);
    }

    #[tokio::test]
    async fn test_async_field_value_completions_for_build_depends() {
        let cache = test_cache();
        let completions =
            get_async_field_value_completions("Build-Depends", "", &cache, &test_arch_list())
                .await
                .expect("Should return completions");
        assert!(!completions.is_empty());
    }

    #[tokio::test]
    async fn test_async_field_value_completions_for_non_relationship() {
        let cache = test_cache();
        let completions =
            get_async_field_value_completions("Homepage", "http", &cache, &test_arch_list()).await;
        assert!(completions.is_none());
    }

    #[tokio::test]
    async fn test_relationship_completions_package_name_empty() {
        let cache = test_cache();
        let completions = get_relationship_completions("", &cache, &test_arch_list()).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"debhelper-compat"));
        assert!(labels.contains(&"cmake"));
    }

    #[tokio::test]
    async fn test_relationship_completions_package_name_prefix() {
        let cache = test_cache();
        let completions = get_relationship_completions("deb", &cache, &test_arch_list()).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["debhelper-compat"]);
    }

    #[tokio::test]
    async fn test_relationship_completions_after_comma() {
        let cache = test_cache();
        let completions =
            get_relationship_completions("libc6 (>= 2.17), cm", &cache, &test_arch_list()).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["cmake"]);
    }

    #[tokio::test]
    async fn test_relationship_completions_after_pipe() {
        let cache = test_cache();
        let completions =
            get_relationship_completions("libfoo | cm", &cache, &test_arch_list()).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["cmake"]);
    }

    #[tokio::test]
    async fn test_relationship_completions_version_operator() {
        let cache = test_cache();
        let completions = get_relationship_completions("libc6 (", &cache, &test_arch_list()).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec![">=", "<=", "=", ">>", "<<"]);
    }

    #[tokio::test]
    async fn test_relationship_completions_version_operator_partial() {
        let cache = test_cache();
        let completions = get_relationship_completions("libc6 (>", &cache, &test_arch_list()).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec![">=", ">>"]);
    }

    #[tokio::test]
    async fn test_relationship_completions_version_position() {
        let cache = test_cache();
        let completions =
            get_relationship_completions("libc6 (>= ", &cache, &test_arch_list()).await;
        assert!(completions.is_empty());
    }

    #[test]
    fn test_relationship_field_detection() {
        assert!(is_relationship_field("Depends"));
        assert!(is_relationship_field("depends"));
        assert!(is_relationship_field("Build-Depends"));
        assert!(is_relationship_field("Pre-Depends"));
        assert!(is_relationship_field("Recommends"));
        assert!(!is_relationship_field("Section"));
        assert!(!is_relationship_field("Priority"));
        assert!(!is_relationship_field("Homepage"));
    }

    #[tokio::test]
    async fn test_relationship_completions_multiline_value() {
        let cache = test_cache();
        let completions =
            get_relationship_completions("libc6 (>= 2.17),\n dh-py", &cache, &test_arch_list())
                .await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["dh-python"]);
    }

    #[tokio::test]
    async fn test_relationship_completions_with_description() {
        let cache = test_cache();
        let completions = get_relationship_completions("cm", &cache, &test_arch_list()).await;
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].label, "cmake");
        assert_eq!(
            completions[0].detail,
            Some("cross-platform make".to_string())
        );
        assert_eq!(
            completions[0].documentation,
            Some(Documentation::String("cross-platform make".to_string()))
        );
    }

    #[tokio::test]
    async fn test_relationship_completions_without_description() {
        let cache = test_cache();
        let completions =
            get_relationship_completions("debhelper", &cache, &test_arch_list()).await;
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].label, "debhelper-compat");
        assert_eq!(completions[0].detail, Some("Package".to_string()));
        assert_eq!(completions[0].documentation, None);
    }

    #[test]
    fn test_determine_relation_position_empty() {
        assert_eq!(
            determine_relation_position(""),
            RelationCompletionPosition::PackageName(String::new())
        );
    }

    #[test]
    fn test_determine_relation_position_leading_space() {
        assert_eq!(
            determine_relation_position(" dh"),
            RelationCompletionPosition::PackageName("dh".to_string())
        );
    }

    #[test]
    fn test_determine_relation_position_leading_space_operator() {
        assert_eq!(
            determine_relation_position(" libc6 ("),
            RelationCompletionPosition::VersionOperator(String::new())
        );
    }

    #[test]
    fn test_determine_relation_position_partial_name() {
        assert_eq!(
            determine_relation_position("deb"),
            RelationCompletionPosition::PackageName("deb".to_string())
        );
    }

    #[test]
    fn test_determine_relation_position_after_open_paren() {
        assert_eq!(
            determine_relation_position("libc6 ("),
            RelationCompletionPosition::VersionOperator(String::new())
        );
    }

    #[test]
    fn test_determine_relation_position_partial_operator() {
        assert_eq!(
            determine_relation_position("libc6 (>"),
            RelationCompletionPosition::VersionOperator(">".to_string())
        );
    }

    #[test]
    fn test_determine_relation_position_after_operator() {
        assert_eq!(
            determine_relation_position("libc6 (>= "),
            RelationCompletionPosition::Version(String::new())
        );
    }

    #[test]
    fn test_determine_relation_position_partial_version() {
        assert_eq!(
            determine_relation_position("libc6 (>= 2.1"),
            RelationCompletionPosition::Version("2.1".to_string())
        );
    }

    #[test]
    fn test_determine_relation_position_after_comma() {
        assert_eq!(
            determine_relation_position("libc6, "),
            RelationCompletionPosition::PackageName(String::new())
        );
    }

    #[test]
    fn test_determine_relation_position_after_complete_relation() {
        assert_eq!(
            determine_relation_position("libc6 (>= 2.17), lib"),
            RelationCompletionPosition::PackageName("lib".to_string())
        );
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

    #[test]
    fn test_determine_relation_position_after_open_bracket() {
        assert_eq!(
            determine_relation_position("libc6 ["),
            RelationCompletionPosition::Architecture(String::new())
        );
    }

    #[test]
    fn test_determine_relation_position_partial_arch() {
        assert_eq!(
            determine_relation_position("libc6 [amd"),
            RelationCompletionPosition::Architecture("amd".to_string())
        );
    }

    #[test]
    fn test_determine_relation_position_second_arch() {
        assert_eq!(
            determine_relation_position("libc6 [amd64 "),
            RelationCompletionPosition::Architecture(String::new())
        );
    }

    #[test]
    fn test_determine_relation_position_negated_arch() {
        assert_eq!(
            determine_relation_position("libc6 [amd64 !arm"),
            RelationCompletionPosition::Architecture("!arm".to_string())
        );
    }
}
