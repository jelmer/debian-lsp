use debian_control::lossless::relations::Relations;
use debian_control::relations::SyntaxKind as RelSyntaxKind;
use rowan::NodeOrToken;
use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Documentation};

use crate::architecture::SharedArchitectureList;
use crate::package_cache::SharedPackageCache;

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
pub(crate) fn is_relationship_field(field_name: &str) -> bool {
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
    /// After `:` on a package name — expecting an architecture qualifier.
    ArchQualifier(String),
    /// Inside `<` — expecting a build profile name.
    BuildProfile(String),
    /// Inside `${` — expecting a substvar name.
    Substvar(String),
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

    // Check if we're inside a SUBSTVAR node (i.e. inside `${...}`).
    let in_substvar = token
        .parent_ancestors()
        .any(|n| n.kind() == RelSyntaxKind::SUBSTVAR);

    if in_substvar {
        let substvar_node = token
            .parent_ancestors()
            .find(|n| n.kind() == RelSyntaxKind::SUBSTVAR)
            .unwrap();

        // Collect the text of IDENT and COLON tokens inside the substvar.
        let partial: String = substvar_node
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

        return RelationCompletionPosition::Substvar(partial);
    }

    // Check if we're inside an ARCHQUAL node (i.e. after ":" on a package name).
    let in_archqual = token
        .parent_ancestors()
        .any(|n| n.kind() == RelSyntaxKind::ARCHQUAL);

    if in_archqual {
        match token.kind() {
            RelSyntaxKind::COLON => {
                return RelationCompletionPosition::ArchQualifier(String::new());
            }
            RelSyntaxKind::IDENT => {
                // If followed by whitespace, the qualifier is complete —
                // we're past the archqual, back to package name position.
                let has_trailing_ws = token.next_token().is_some_and(|t| {
                    matches!(t.kind(), RelSyntaxKind::WHITESPACE | RelSyntaxKind::NEWLINE)
                });
                if has_trailing_ws {
                    return RelationCompletionPosition::PackageName(String::new());
                }
                return RelationCompletionPosition::ArchQualifier(token.text().to_string());
            }
            _ => {
                return RelationCompletionPosition::ArchQualifier(String::new());
            }
        }
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

    // Check if we're inside a PROFILES node (i.e. inside angle brackets).
    let in_profiles = token
        .parent_ancestors()
        .any(|n| n.kind() == RelSyntaxKind::PROFILES);

    if in_profiles {
        match token.kind() {
            RelSyntaxKind::L_ANGLE => {
                return RelationCompletionPosition::BuildProfile(String::new());
            }
            RelSyntaxKind::IDENT => {
                let has_trailing_ws = token.next_token().is_some_and(|t| {
                    matches!(t.kind(), RelSyntaxKind::WHITESPACE | RelSyntaxKind::NEWLINE)
                });
                if has_trailing_ws {
                    return RelationCompletionPosition::BuildProfile(String::new());
                }
                let negated = token
                    .prev_token()
                    .is_some_and(|t| t.kind() == RelSyntaxKind::NOT);
                let prefix = if negated {
                    format!("!{}", token.text())
                } else {
                    token.text().to_string()
                };
                return RelationCompletionPosition::BuildProfile(prefix);
            }
            RelSyntaxKind::NOT => {
                return RelationCompletionPosition::BuildProfile("!".to_string());
            }
            _ => {
                return RelationCompletionPosition::BuildProfile(String::new());
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
        RelationCompletionPosition::ArchQualifier(partial) => {
            get_arch_qualifier_completions(&partial, architecture_list).await
        }
        RelationCompletionPosition::BuildProfile(partial) => {
            get_build_profile_completions(&partial)
        }
        RelationCompletionPosition::Substvar(partial) => get_substvar_completions(&partial),
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

/// Special architecture qualifier values.
const ARCH_QUALIFIER_SPECIALS: &[(&str, &str)] = &[
    ("any", "Satisfied by any architecture"),
    ("native", "Host architecture only"),
];

/// Get completion items for architecture qualifiers (after `:` on a package name).
///
/// Offers the special qualifiers `any` and `native`, plus all known architecture names.
async fn get_arch_qualifier_completions(
    partial: &str,
    architecture_list: &SharedArchitectureList,
) -> Vec<CompletionItem> {
    let mut completions: Vec<CompletionItem> = ARCH_QUALIFIER_SPECIALS
        .iter()
        .filter(|(name, _)| name.starts_with(partial))
        .map(|&(name, desc)| CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some(desc.to_string()),
            ..Default::default()
        })
        .collect();

    let arches = architecture_list.read().await;
    completions.extend(
        arches
            .iter()
            .filter(|arch| arch.starts_with(partial))
            .map(|arch| CompletionItem {
                label: arch.clone(),
                kind: Some(CompletionItemKind::VALUE),
                detail: Some("Specific architecture".to_string()),
                ..Default::default()
            }),
    );

    completions
}

/// Known build profile names.
///
/// See <https://wiki.debian.org/BuildProfileSpec> and dpkg's
/// `vendor/default/tupletable`.
const BUILD_PROFILES: &[(&str, &str)] = &[
    ("cross", "Cross-compilation mode"),
    ("nobiarch", "Disable multiarch/biarch support"),
    ("nocheck", "Skip test suites"),
    ("nodoc", "Skip documentation generation"),
    ("nogolang", "Skip Go-related build steps"),
    ("noinsttest", "Skip installed tests"),
    ("noperl", "Skip Perl-related build steps"),
    ("nopython", "Skip Python-related build steps"),
    ("noruby", "Skip Ruby-related build steps"),
    ("notriggered", "Do not activate triggers"),
    ("stage1", "Bootstrap stage 1"),
    ("stage2", "Bootstrap stage 2"),
];

/// Get completion items for build profiles (inside `<...>`).
fn get_build_profile_completions(partial: &str) -> Vec<CompletionItem> {
    let negated = partial.starts_with('!');
    let prefix = if negated { &partial[1..] } else { partial };

    BUILD_PROFILES
        .iter()
        .filter(|(name, _)| name.starts_with(prefix))
        .map(|&(name, desc)| {
            let label = if negated {
                format!("!{}", name)
            } else {
                name.to_string()
            };
            CompletionItem {
                label,
                kind: Some(CompletionItemKind::VALUE),
                detail: Some(desc.to_string()),
                ..Default::default()
            }
        })
        .collect()
}

/// Known substitution variables used in relationship fields.
///
/// See deb-substvars(5).
const KNOWN_SUBSTVARS: &[(&str, &str)] = &[
    ("shlibs:Depends", "Shared library dependencies"),
    ("shlibs:Pre-Depends", "Shared library pre-dependencies"),
    ("shlibs:Suggests", "Shared library suggestions"),
    ("shlibs:Recommends", "Shared library recommendations"),
    ("misc:Depends", "Miscellaneous dependencies (debhelper)"),
    (
        "misc:Pre-Depends",
        "Miscellaneous pre-dependencies (debhelper)",
    ),
    (
        "misc:Recommends",
        "Miscellaneous recommendations (debhelper)",
    ),
    ("misc:Suggests", "Miscellaneous suggestions (debhelper)"),
    ("misc:Breaks", "Miscellaneous breaks (debhelper)"),
    ("misc:Enhances", "Miscellaneous enhances (debhelper)"),
    ("misc:Provides", "Miscellaneous provides (debhelper)"),
    ("misc:Conflicts", "Miscellaneous conflicts (debhelper)"),
    ("misc:Replaces", "Miscellaneous replaces (debhelper)"),
    ("perl:Depends", "Perl dependencies (dh_perl)"),
    ("python3:Depends", "Python 3 dependencies (dh_python3)"),
    ("python3:Provides", "Python 3 provides (dh_python3)"),
    ("python3:Breaks", "Python 3 breaks (dh_python3)"),
    (
        "sphinxdoc:Depends",
        "Sphinx documentation dependencies (dh_sphinxdoc)",
    ),
    ("binary:Version", "Current binary package version"),
    ("source:Version", "Current source package version"),
    (
        "source:Upstream-Version",
        "Upstream version (without Debian revision)",
    ),
];

/// Get completion items for substitution variables (inside `${...}`).
fn get_substvar_completions(partial: &str) -> Vec<CompletionItem> {
    KNOWN_SUBSTVARS
        .iter()
        .filter(|(name, _)| name.starts_with(partial))
        .map(|&(name, desc)| CompletionItem {
            label: format!("${{{}}}", name),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some(desc.to_string()),
            // Insert just the name part — the `${` is already typed and `}`
            // will be added or is already present.
            insert_text: Some(format!("{}}}", name)),
            ..Default::default()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package_cache::TestPackageCache;
    use std::sync::Arc;

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

    #[test]
    fn test_determine_relation_position_after_colon() {
        assert_eq!(
            determine_relation_position("libc6:"),
            RelationCompletionPosition::ArchQualifier(String::new())
        );
    }

    #[test]
    fn test_determine_relation_position_partial_archqual() {
        assert_eq!(
            determine_relation_position("libc6:an"),
            RelationCompletionPosition::ArchQualifier("an".to_string())
        );
    }

    #[test]
    fn test_determine_relation_position_complete_archqual() {
        assert_eq!(
            determine_relation_position("libc6:any "),
            RelationCompletionPosition::PackageName(String::new())
        );
    }

    #[tokio::test]
    async fn test_arch_qualifier_completions_empty() {
        let cache = test_cache();
        let arch_list = test_arch_list();
        let completions = get_relationship_completions("libc6:", &cache, &arch_list).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"any"));
        assert!(labels.contains(&"native"));
        assert!(labels.contains(&"amd64"));
    }

    #[tokio::test]
    async fn test_arch_qualifier_completions_partial() {
        let cache = test_cache();
        let arch_list = test_arch_list();
        let completions = get_relationship_completions("libc6:a", &cache, &arch_list).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"any"));
        assert!(labels.contains(&"amd64"));
        assert!(labels.contains(&"arm64"));
        assert!(labels.contains(&"armhf"));
        assert!(!labels.contains(&"native"));
        assert!(!labels.contains(&"i386"));
    }

    #[test]
    fn test_determine_relation_position_after_angle_bracket() {
        assert_eq!(
            determine_relation_position("libc6 <"),
            RelationCompletionPosition::BuildProfile(String::new())
        );
    }

    #[test]
    fn test_determine_relation_position_partial_profile() {
        assert_eq!(
            determine_relation_position("libc6 <no"),
            RelationCompletionPosition::BuildProfile("no".to_string())
        );
    }

    #[test]
    fn test_determine_relation_position_negated_profile() {
        assert_eq!(
            determine_relation_position("libc6 <!no"),
            RelationCompletionPosition::BuildProfile("!no".to_string())
        );
    }

    #[test]
    fn test_determine_relation_position_second_profile() {
        assert_eq!(
            determine_relation_position("libc6 <cross "),
            RelationCompletionPosition::BuildProfile(String::new())
        );
    }

    #[test]
    fn test_determine_relation_position_profile_after_arch() {
        assert_eq!(
            determine_relation_position("libc6 [amd64] <"),
            RelationCompletionPosition::BuildProfile(String::new())
        );
    }

    #[test]
    fn test_build_profile_completions_empty() {
        let completions = get_build_profile_completions("");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"cross"));
        assert!(labels.contains(&"nocheck"));
        assert!(labels.contains(&"stage1"));
    }

    #[test]
    fn test_build_profile_completions_partial() {
        let completions = get_build_profile_completions("no");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"nocheck"));
        assert!(labels.contains(&"nodoc"));
        assert!(!labels.contains(&"cross"));
        assert!(!labels.contains(&"stage1"));
    }

    #[test]
    fn test_build_profile_completions_negated() {
        let completions = get_build_profile_completions("!no");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"!nocheck"));
        assert!(labels.contains(&"!nodoc"));
        assert!(!labels.contains(&"!cross"));
    }

    #[tokio::test]
    async fn test_relationship_completions_build_profile() {
        let cache = test_cache();
        let arch_list = test_arch_list();
        let completions = get_relationship_completions("libc6 <no", &cache, &arch_list).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"nocheck"));
        assert!(!labels.contains(&"cross"));
    }

    #[test]
    fn test_determine_relation_position_after_dollar_brace() {
        assert_eq!(
            determine_relation_position("${"),
            RelationCompletionPosition::Substvar(String::new())
        );
    }

    #[test]
    fn test_determine_relation_position_partial_substvar() {
        assert_eq!(
            determine_relation_position("${shlibs"),
            RelationCompletionPosition::Substvar("shlibs".to_string())
        );
    }

    #[test]
    fn test_determine_relation_position_substvar_with_colon() {
        assert_eq!(
            determine_relation_position("${shlibs:Dep"),
            RelationCompletionPosition::Substvar("shlibs:Dep".to_string())
        );
    }

    #[test]
    fn test_substvar_completions_empty() {
        let completions = get_substvar_completions("");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"${shlibs:Depends}"));
        assert!(labels.contains(&"${misc:Depends}"));
    }

    #[test]
    fn test_substvar_completions_partial() {
        let completions = get_substvar_completions("shlibs");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"${shlibs:Depends}"));
        assert!(!labels.contains(&"${misc:Depends}"));
    }

    #[test]
    fn test_substvar_completions_with_colon() {
        let completions = get_substvar_completions("misc:D");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"${misc:Depends}"));
        assert!(!labels.contains(&"${misc:Recommends}"));
    }

    #[tokio::test]
    async fn test_relationship_completions_substvar() {
        let cache = test_cache();
        let arch_list = test_arch_list();
        let completions = get_relationship_completions("${shlibs:D", &cache, &arch_list).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"${shlibs:Depends}"));
        assert!(!labels.contains(&"${misc:Depends}"));
    }

    #[tokio::test]
    async fn test_relationship_completions_substvar_after_comma() {
        let cache = test_cache();
        let arch_list = test_arch_list();
        let completions = get_relationship_completions("libc6, ${misc", &cache, &arch_list).await;
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"${misc:Depends}"));
    }
}
