//! Inlay hints for debian/control files.
//!
//! Shows whether the Standards-Version is current or outdated:
//!   Standards-Version: 4.6.2   [latest: 4.7.0]
//!
//! Shows whether the debhelper compat level is current:
//!   debhelper-compat (= 13)   [current: 14]

use std::collections::HashMap;

use debian_control::lossless::relations::Relations;
use debian_control::relations::VersionConstraint;
use text_size::TextSize;
use tower_lsp_server::ls_types::{InlayHint, InlayHintKind, InlayHintLabel};

use crate::position::text_range_to_lsp_range;

/// Extract the Standards-Version (first 3 components) from a debian-policy
/// package version string like "4.7.3.0" → "4.7.3".
fn policy_version_to_standards_version(policy_version: &str) -> Option<&str> {
    let mut dots = 0;
    for (i, c) in policy_version.char_indices() {
        if c == '.' {
            dots += 1;
            if dots == 3 {
                return Some(&policy_version[..i]);
            }
        }
    }
    // If there are fewer than 3 dots, return the whole version
    if dots >= 2 {
        Some(policy_version)
    } else {
        None
    }
}

/// Info about a Standards-Version field found in the control file.
struct StandardsVersionInfo {
    /// The value of the Standards-Version field (trimmed).
    value: String,
    /// The end position of the value in the source text.
    value_end: TextSize,
}

/// Find Standards-Version fields in a parsed control file within the given range.
fn find_standards_versions(
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    source_text: &str,
    range: &tower_lsp_server::ls_types::Range,
) -> Vec<StandardsVersionInfo> {
    let control = parsed.tree();

    let Some(text_range) = crate::position::try_lsp_range_to_text_range(source_text, range) else {
        return Vec::new();
    };

    let mut results = Vec::new();

    for paragraph in control.as_deb822().paragraphs() {
        for entry in paragraph.entries() {
            let entry_range = entry.text_range();

            if entry_range.start() >= text_range.end() || entry_range.end() <= text_range.start() {
                continue;
            }

            let Some(field_name) = entry.key() else {
                continue;
            };

            if !field_name.eq_ignore_ascii_case("Standards-Version") {
                continue;
            }

            let value = entry.value();
            let value = value.trim().to_string();
            if value.is_empty() {
                continue;
            }

            let Some(value_range) = entry.value_range() else {
                continue;
            };

            results.push(StandardsVersionInfo {
                value,
                value_end: value_range.end(),
            });
        }
    }

    results
}

/// Build-Depends field names that may contain debhelper-compat.
const BUILD_DEPENDS_FIELDS: &[&str] =
    &["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"];

/// Info about a debhelper-compat relation found in the control file.
struct DebhelperCompatInfo {
    /// The end position of the relation (after closing paren) in the source text.
    relation_end: TextSize,
}

/// Extract the major version (compat level) from a debhelper package version
/// string like "13.31" → 13.
fn debhelper_version_to_compat_level(version: &str) -> Option<u32> {
    let major = version.split('.').next()?;
    // Strip any epoch prefix (e.g. "1:13" → "13")
    let major = major.rsplit(':').next()?;
    major.parse().ok()
}

/// Map an offset in the joined value string (as produced by `entry.value()`)
/// back to an absolute source position using the individual VALUE token ranges.
///
/// `entry.value()` joins VALUE tokens with `\n`, so for multi-line values the
/// offsets in the joined string don't correspond 1:1 to source positions.
fn joined_offset_to_source_offset(
    line_ranges: &[text_size::TextRange],
    joined_offset: usize,
) -> Option<TextSize> {
    let mut remaining = joined_offset;
    for (i, lr) in line_ranges.iter().enumerate() {
        let line_len: usize = lr.len().into();
        if remaining <= line_len {
            return Some(lr.start() + TextSize::from(remaining as u32));
        }
        remaining -= line_len;
        // Account for the '\n' separator that entry.value() inserts
        // between VALUE tokens (except after the last one)
        if i < line_ranges.len() - 1 {
            if remaining == 0 {
                // The offset points exactly at the '\n' separator;
                // map to the end of this line
                return Some(lr.end());
            }
            remaining -= 1; // skip the '\n'
        }
    }
    None
}

/// Find debhelper-compat relations in a parsed control file within the given range.
fn find_debhelper_compat(
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    source_text: &str,
    range: &tower_lsp_server::ls_types::Range,
) -> Vec<DebhelperCompatInfo> {
    let control = parsed.tree();

    let Some(text_range) = crate::position::try_lsp_range_to_text_range(source_text, range) else {
        return Vec::new();
    };

    let mut results = Vec::new();

    for paragraph in control.as_deb822().paragraphs() {
        for entry in paragraph.entries() {
            let entry_range = entry.text_range();

            if entry_range.start() >= text_range.end() || entry_range.end() <= text_range.start() {
                continue;
            }

            let Some(field_name) = entry.key() else {
                continue;
            };

            if !BUILD_DEPENDS_FIELDS
                .iter()
                .any(|f| f.eq_ignore_ascii_case(&field_name))
            {
                continue;
            }

            let value = entry.value();
            let (relations, _errors) = Relations::parse_relaxed(&value, true);

            // Build a mapping from offsets in the joined value string
            // to absolute source positions. entry.value() joins VALUE
            // tokens with '\n', stripping INDENT/NEWLINE tokens, so
            // positions don't map 1:1 for multi-line values.
            let line_ranges = entry.value_line_ranges();

            for rel_entry in relations.entries() {
                for relation in rel_entry.relations() {
                    if relation.try_name().as_deref() != Some("debhelper-compat") {
                        continue;
                    }

                    let Some((VersionConstraint::Equal, _)) = relation.version() else {
                        continue;
                    };

                    let rel_end: usize = relation.syntax().text_range().end().into();
                    let Some(absolute_end) = joined_offset_to_source_offset(&line_ranges, rel_end)
                    else {
                        continue;
                    };

                    results.push(DebhelperCompatInfo {
                        relation_end: absolute_end,
                    });
                }
            }
        }
    }

    results
}

/// Info about a relation in a dependency field.
struct RelationInfo {
    /// The package name.
    name: String,
    /// The end position of the relation in the source text.
    relation_end: TextSize,
}

/// Info about a substvar in a dependency field.
struct SubstvarInfo {
    /// The substvar name (e.g. "binary:Version").
    name: String,
    /// The end position of the substvar in the source text.
    substvar_end: TextSize,
}

/// Find all relations and substvars in relationship fields within the given range.
fn find_relations(
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    source_text: &str,
    range: &tower_lsp_server::ls_types::Range,
) -> (Vec<RelationInfo>, Vec<SubstvarInfo>) {
    let control = parsed.tree();

    let Some(text_range) = crate::position::try_lsp_range_to_text_range(source_text, range) else {
        return (Vec::new(), Vec::new());
    };

    let mut rel_results = Vec::new();
    let mut sv_results = Vec::new();

    for paragraph in control.as_deb822().paragraphs() {
        for entry in paragraph.entries() {
            let entry_range = entry.text_range();

            if entry_range.start() >= text_range.end() || entry_range.end() <= text_range.start() {
                continue;
            }

            let Some(field_name) = entry.key() else {
                continue;
            };

            if !super::relation_completion::is_relationship_field(&field_name) {
                continue;
            }

            let value = entry.value();
            let (parsed_rels, _errors) = Relations::parse_relaxed(&value, true);
            let line_ranges = entry.value_line_ranges();

            for rel_entry in parsed_rels.entries() {
                for relation in rel_entry.relations() {
                    let Some(name) = relation.try_name() else {
                        continue;
                    };

                    let rel_end: usize = relation.syntax().text_range().end().into();
                    let Some(absolute_end) = joined_offset_to_source_offset(&line_ranges, rel_end)
                    else {
                        continue;
                    };

                    rel_results.push(RelationInfo {
                        name,
                        relation_end: absolute_end,
                    });
                }
            }

            for sv in parsed_rels.substvar_nodes() {
                let sv_end: usize = sv.syntax().text_range().end().into();
                let Some(absolute_end) = joined_offset_to_source_offset(&line_ranges, sv_end)
                else {
                    continue;
                };
                // The substvar text is "${name}", strip the delimiters
                let raw = sv.to_string();
                let name = raw
                    .strip_prefix("${")
                    .and_then(|s| s.strip_suffix('}'))
                    .unwrap_or(&raw)
                    .to_string();
                sv_results.push(SubstvarInfo {
                    name,
                    substvar_end: absolute_end,
                });
            }
        }
    }

    (rel_results, sv_results)
}

/// Format a compact version hint from cached version info.
///
/// Examples:
///   `[sid,trixie: 13.31 | bullseye: 13.3.4]`
///   `[sid,trixie: 13.31]` (single version across all suites)
///   `[available: 13.31]` (no suite info)
fn format_version_hint(versions: &[crate::package_cache::VersionInfo]) -> Option<String> {
    if versions.is_empty() {
        return None;
    }

    // Check if any version has suite info
    let has_suites = versions.iter().any(|v| !v.suites.is_empty());

    if !has_suites {
        // No suite info — just show the candidate version
        return Some(format!("[available: {}]", versions[0].version));
    }

    // Group by version, show "suite1,suite2: version" for each
    let parts: Vec<String> = versions
        .iter()
        .filter(|v| !v.suites.is_empty())
        .map(|v| format!("{}: {}", v.suites.join(","), v.version))
        .collect();

    if parts.is_empty() {
        return Some(format!("[available: {}]", versions[0].version));
    }

    Some(format!("[{}]", parts.join(" | ")))
}

/// Generate inlay hints for a control file.
///
/// Currently provides hints for:
/// - Standards-Version: shows `[latest: X.Y.Z]` if outdated
/// - debhelper-compat (= N): shows `[current: M]`
/// - Archive versions: shows `[available: X.Y.Z]` for real packages
/// - Virtual packages: shows `[-> provider1 | provider2 | ...]`
/// - Substvars: shows `[= value]` for known substitution variables
///
/// The `resolved_substvars` map provides values for substvar names
/// (e.g. `"binary:Version"` → `"1.2.3-1"`).
///
/// Returns `(hints, uncached_packages)`. The caller should load versions and
/// providers for uncached packages in the background and then send
/// `workspace/inlayHint/refresh` to the client.
pub async fn generate_inlay_hints(
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    source_text: &str,
    range: &tower_lsp_server::ls_types::Range,
    package_cache: &crate::package_cache::SharedPackageCache,
    resolved_substvars: &HashMap<String, String>,
) -> (Vec<InlayHint>, Vec<String>) {
    // Extract info synchronously (CST types are not Send)
    let sv_entries = find_standards_versions(parsed, source_text, range);
    let dh_entries = find_debhelper_compat(parsed, source_text, range);
    let (rel_entries, substvar_entries) = find_relations(parsed, source_text, range);

    if sv_entries.is_empty()
        && dh_entries.is_empty()
        && rel_entries.is_empty()
        && substvar_entries.is_empty()
    {
        return (Vec::new(), Vec::new());
    }

    let mut hints = Vec::new();

    // Standards-Version hints
    if !sv_entries.is_empty() {
        let latest_standards = {
            let mut cache = package_cache.write().await;
            let versions = cache.load_versions("debian-policy").await;
            versions.and_then(|vs| {
                vs.first()
                    .and_then(|v| policy_version_to_standards_version(&v.version))
                    .map(|s| s.to_string())
            })
        };

        if let Some(latest) = latest_standards {
            for sv in &sv_entries {
                if sv.value == latest || !is_outdated(&sv.value, &latest) {
                    continue;
                }

                let lsp_range = text_range_to_lsp_range(
                    source_text,
                    text_size::TextRange::new(sv.value_end, sv.value_end),
                );

                hints.push(InlayHint {
                    position: lsp_range.start,
                    label: InlayHintLabel::String(format!("[latest: {}]", latest)),
                    kind: Some(InlayHintKind::TYPE),
                    text_edits: None,
                    tooltip: None,
                    padding_left: Some(true),
                    padding_right: None,
                    data: None,
                });
            }
        }
    }

    // debhelper-compat hints
    if !dh_entries.is_empty() {
        let latest_compat = {
            let mut cache = package_cache.write().await;
            let versions = cache.load_versions("debhelper").await;
            versions.and_then(|vs| {
                vs.first()
                    .and_then(|v| debhelper_version_to_compat_level(&v.version))
            })
        };

        if let Some(latest) = latest_compat {
            for dh in &dh_entries {
                let lsp_range = text_range_to_lsp_range(
                    source_text,
                    text_size::TextRange::new(dh.relation_end, dh.relation_end),
                );

                hints.push(InlayHint {
                    position: lsp_range.start,
                    label: InlayHintLabel::String(format!("[current: {}]", latest)),
                    kind: Some(InlayHintKind::TYPE),
                    text_edits: None,
                    tooltip: None,
                    padding_left: Some(true),
                    padding_right: None,
                    data: None,
                });
            }
        }
    }

    // Per-relation hints (archive versions, virtual package providers).
    // Use only cached data (read lock) to avoid blocking the LSP response.
    // Returns uncached package names so the caller can trigger background
    // loading and an inlayHint/refresh.
    let mut uncached_packages = Vec::new();
    {
        let cache = package_cache.read().await;
        for rel in &rel_entries {
            let cached_versions = cache.get_cached_versions(&rel.name);
            let cached_providers = cache.get_cached_providers(&rel.name);

            if let Some(versions) = cached_versions {
                if !versions.is_empty() {
                    // Real package with version info — show archive versions
                    if let Some(label) = format_version_hint(versions) {
                        let lsp_range = text_range_to_lsp_range(
                            source_text,
                            text_size::TextRange::new(rel.relation_end, rel.relation_end),
                        );
                        hints.push(InlayHint {
                            position: lsp_range.start,
                            label: InlayHintLabel::String(label),
                            kind: Some(InlayHintKind::TYPE),
                            text_edits: None,
                            tooltip: None,
                            padding_left: Some(true),
                            padding_right: None,
                            data: None,
                        });
                    }
                } else if let Some(providers) = cached_providers {
                    // Versions cached but empty = virtual package; show providers
                    if !providers.is_empty() {
                        let lsp_range = text_range_to_lsp_range(
                            source_text,
                            text_size::TextRange::new(rel.relation_end, rel.relation_end),
                        );

                        let label = if providers.len() <= 3 {
                            format!("[-> {}]", providers.join(" | "))
                        } else {
                            format!("[-> {} | ...]", providers[..3].join(" | "))
                        };

                        hints.push(InlayHint {
                            position: lsp_range.start,
                            label: InlayHintLabel::String(label),
                            kind: Some(InlayHintKind::TYPE),
                            text_edits: None,
                            tooltip: None,
                            padding_left: Some(true),
                            padding_right: None,
                            data: None,
                        });
                    }
                }
                // else: versions empty, no providers cached — will be loaded in background
            } else {
                // Versions not cached yet
                uncached_packages.push(rel.name.clone());
            }
        }
    }

    // Substvar hints
    for sv in &substvar_entries {
        if let Some(value) = resolved_substvars.get(&sv.name) {
            let lsp_range = text_range_to_lsp_range(
                source_text,
                text_size::TextRange::new(sv.substvar_end, sv.substvar_end),
            );

            hints.push(InlayHint {
                position: lsp_range.start,
                label: InlayHintLabel::String(format!("[= {}]", value)),
                kind: Some(InlayHintKind::TYPE),
                text_edits: None,
                tooltip: None,
                padding_left: Some(true),
                padding_right: None,
                data: None,
            });
        }
    }

    (hints, uncached_packages)
}

/// Compare two dotted version strings and return true if `current` is older
/// than `latest`.
fn is_outdated(current: &str, latest: &str) -> bool {
    let current_parts: Vec<u32> = current.split('.').filter_map(|s| s.parse().ok()).collect();
    let latest_parts: Vec<u32> = latest.split('.').filter_map(|s| s.parse().ok()).collect();

    for (c, l) in current_parts.iter().zip(latest_parts.iter()) {
        match c.cmp(l) {
            std::cmp::Ordering::Less => return true,
            std::cmp::Ordering::Greater => return false,
            std::cmp::Ordering::Equal => continue,
        }
    }

    // If all compared parts are equal, the one with fewer parts is "older"
    // e.g. 4.7 < 4.7.3
    current_parts.len() < latest_parts.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_version_to_standards_version() {
        assert_eq!(
            policy_version_to_standards_version("4.7.3.0"),
            Some("4.7.3")
        );
        assert_eq!(policy_version_to_standards_version("4.7.3"), Some("4.7.3"));
        assert_eq!(
            policy_version_to_standards_version("4.7.3.0.1"),
            Some("4.7.3")
        );
        assert_eq!(policy_version_to_standards_version("4.7"), None);
        assert_eq!(policy_version_to_standards_version("4"), None);
    }

    #[test]
    fn test_is_outdated() {
        assert!(is_outdated("4.6.2", "4.7.0"));
        assert!(is_outdated("4.6.2", "4.6.3"));
        assert!(is_outdated("4.6.2", "5.0.0"));
        assert!(is_outdated("4.6", "4.6.3"));
        assert!(!is_outdated("4.7.0", "4.7.0"));
        assert!(!is_outdated("4.7.1", "4.7.0"));
        assert!(!is_outdated("5.0.0", "4.7.0"));
    }

    #[tokio::test]
    async fn test_inlay_hint_outdated_standards_version() {
        use crate::package_cache::{TestPackageCache, VersionInfo};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let mut cache = TestPackageCache::default();
        cache.versions.insert(
            "debian-policy".to_string(),
            vec![VersionInfo {
                version: "4.7.3.0".to_string(),
                suites: vec!["unstable".to_string()],
            }],
        );
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));

        let content =
            "Source: test-package\nStandards-Version: 4.6.2\nMaintainer: Test <test@example.com>\n";
        let parsed = debian_control::lossless::Control::parse(content);
        let range = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position::new(0, 0),
            end: tower_lsp_server::ls_types::Position::new(3, 0),
        };

        let (hints, _uncached) =
            generate_inlay_hints(&parsed, content, &range, &shared_cache, &HashMap::new()).await;

        assert_eq!(hints.len(), 1);
        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "[latest: 4.7.3]"),
            _ => panic!("Expected string label"),
        }
    }

    #[tokio::test]
    async fn test_no_inlay_hint_when_current() {
        use crate::package_cache::{TestPackageCache, VersionInfo};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let mut cache = TestPackageCache::default();
        cache.versions.insert(
            "debian-policy".to_string(),
            vec![VersionInfo {
                version: "4.7.3.0".to_string(),
                suites: vec!["unstable".to_string()],
            }],
        );
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));

        let content =
            "Source: test-package\nStandards-Version: 4.7.3\nMaintainer: Test <test@example.com>\n";
        let parsed = debian_control::lossless::Control::parse(content);
        let range = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position::new(0, 0),
            end: tower_lsp_server::ls_types::Position::new(3, 0),
        };

        let (hints, _uncached) =
            generate_inlay_hints(&parsed, content, &range, &shared_cache, &HashMap::new()).await;

        assert_eq!(hints.len(), 0);
    }

    #[test]
    fn test_debhelper_version_to_compat_level() {
        assert_eq!(debhelper_version_to_compat_level("13.31"), Some(13));
        assert_eq!(debhelper_version_to_compat_level("14.0"), Some(14));
        assert_eq!(debhelper_version_to_compat_level("13"), Some(13));
        assert_eq!(debhelper_version_to_compat_level("13.3.4"), Some(13));
    }

    #[tokio::test]
    async fn test_inlay_hint_outdated_debhelper_compat() {
        use crate::package_cache::{TestPackageCache, VersionInfo};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let mut cache = TestPackageCache::default();
        cache.versions.insert(
            "debhelper".to_string(),
            vec![VersionInfo {
                version: "14.2".to_string(),
                suites: vec!["unstable".to_string()],
            }],
        );
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));

        let content = "\
Source: test-package
Build-Depends: debhelper-compat (= 13), pkg-config
Maintainer: Test <test@example.com>
";
        let parsed = debian_control::lossless::Control::parse(content);
        let range = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position::new(0, 0),
            end: tower_lsp_server::ls_types::Position::new(3, 0),
        };

        let (hints, _uncached) =
            generate_inlay_hints(&parsed, content, &range, &shared_cache, &HashMap::new()).await;

        assert_eq!(hints.len(), 1);
        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "[current: 14]"),
            _ => panic!("Expected string label"),
        }
    }

    #[tokio::test]
    async fn test_inlay_hint_when_compat_current() {
        use crate::package_cache::{TestPackageCache, VersionInfo};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let mut cache = TestPackageCache::default();
        cache.versions.insert(
            "debhelper".to_string(),
            vec![VersionInfo {
                version: "13.31".to_string(),
                suites: vec!["unstable".to_string()],
            }],
        );
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));

        let content = "\
Source: test-package
Build-Depends: debhelper-compat (= 13), pkg-config
Maintainer: Test <test@example.com>
";
        let parsed = debian_control::lossless::Control::parse(content);
        let range = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position::new(0, 0),
            end: tower_lsp_server::ls_types::Position::new(3, 0),
        };

        let (hints, _uncached) =
            generate_inlay_hints(&parsed, content, &range, &shared_cache, &HashMap::new()).await;

        assert_eq!(hints.len(), 1);
        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "[current: 13]"),
            _ => panic!("Expected string label"),
        }
    }

    #[tokio::test]
    async fn test_both_standards_version_and_debhelper_compat_hints() {
        use crate::package_cache::{TestPackageCache, VersionInfo};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let mut cache = TestPackageCache::default();
        cache.versions.insert(
            "debian-policy".to_string(),
            vec![VersionInfo {
                version: "4.7.3.0".to_string(),
                suites: vec!["unstable".to_string()],
            }],
        );
        cache.versions.insert(
            "debhelper".to_string(),
            vec![VersionInfo {
                version: "14.2".to_string(),
                suites: vec!["unstable".to_string()],
            }],
        );
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));

        let content = "\
Source: test-package
Standards-Version: 4.6.2
Build-Depends: debhelper-compat (= 13), pkg-config
Maintainer: Test <test@example.com>
";
        let parsed = debian_control::lossless::Control::parse(content);
        let range = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position::new(0, 0),
            end: tower_lsp_server::ls_types::Position::new(4, 0),
        };

        let (hints, _uncached) =
            generate_inlay_hints(&parsed, content, &range, &shared_cache, &HashMap::new()).await;

        assert_eq!(hints.len(), 2);
        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "[latest: 4.7.3]"),
            _ => panic!("Expected string label"),
        }
        match &hints[1].label {
            InlayHintLabel::String(s) => assert_eq!(s, "[current: 14]"),
            _ => panic!("Expected string label"),
        }
    }

    #[tokio::test]
    async fn test_inlay_hint_multiline_build_depends() {
        use crate::package_cache::{TestPackageCache, VersionInfo};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let mut cache = TestPackageCache::default();
        cache.versions.insert(
            "debhelper".to_string(),
            vec![VersionInfo {
                version: "14.2".to_string(),
                suites: vec!["unstable".to_string()],
            }],
        );
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));

        let content = "\
Source: test-package
Build-Depends: debhelper (>= 13.5),
               debhelper-compat (= 13),
               pkg-config
Maintainer: Test <test@example.com>
";
        let parsed = debian_control::lossless::Control::parse(content);
        let range = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position::new(0, 0),
            end: tower_lsp_server::ls_types::Position::new(5, 0),
        };

        let (hints, _uncached) =
            generate_inlay_hints(&parsed, content, &range, &shared_cache, &HashMap::new()).await;

        // Expect debhelper-compat hint + debhelper version hint
        assert_eq!(hints.len(), 2);
        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "[current: 14]"),
            _ => panic!("Expected string label"),
        }
        // The compat hint should be on line 2 (the debhelper-compat line)
        assert_eq!(hints[0].position.line, 2);
        match &hints[1].label {
            InlayHintLabel::String(s) => assert_eq!(s, "[unstable: 14.2]"),
            _ => panic!("Expected string label"),
        }
    }

    #[tokio::test]
    async fn test_inlay_hint_virtual_package() {
        use crate::package_cache::TestPackageCache;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let mut cache = TestPackageCache::default();
        // default-mta is virtual: empty versions, has providers
        cache.versions.insert("default-mta".to_string(), Vec::new());
        cache.providers.insert(
            "default-mta".to_string(),
            vec![
                "exim4-daemon-light".to_string(),
                "postfix".to_string(),
                "sendmail-bin".to_string(),
            ],
        );
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));

        let content = "\
Source: test-package

Package: test-package
Depends: default-mta, libc6
Description: A test
";
        let parsed = debian_control::lossless::Control::parse(content);
        let range = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position::new(0, 0),
            end: tower_lsp_server::ls_types::Position::new(5, 0),
        };

        let (hints, _uncached) =
            generate_inlay_hints(&parsed, content, &range, &shared_cache, &HashMap::new()).await;

        assert_eq!(hints.len(), 1);
        match &hints[0].label {
            InlayHintLabel::String(s) => {
                assert_eq!(s, "[-> exim4-daemon-light | postfix | sendmail-bin]")
            }
            _ => panic!("Expected string label"),
        }
    }

    #[tokio::test]
    async fn test_no_provider_hint_for_real_package() {
        use crate::package_cache::{TestPackageCache, VersionInfo};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let mut cache = TestPackageCache::default();
        // libc6 is a real package with versions AND providers
        cache
            .packages
            .push(("libc6".to_string(), Some("C library".to_string())));
        cache.versions.insert(
            "libc6".to_string(),
            vec![VersionInfo {
                version: "2.40-4".to_string(),
                suites: vec!["unstable".to_string()],
            }],
        );
        cache
            .providers
            .insert("libc6".to_string(), vec!["libc6-udeb".to_string()]);
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));

        let content = "\
Source: test-package

Package: test-package
Depends: libc6
Description: A test
";
        let parsed = debian_control::lossless::Control::parse(content);
        let range = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position::new(0, 0),
            end: tower_lsp_server::ls_types::Position::new(5, 0),
        };

        let (hints, _uncached) =
            generate_inlay_hints(&parsed, content, &range, &shared_cache, &HashMap::new()).await;

        // Should show version hint, NOT provider hint
        assert_eq!(hints.len(), 1);
        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "[unstable: 2.40-4]"),
            _ => panic!("Expected string label"),
        }
    }

    #[tokio::test]
    async fn test_inlay_hint_virtual_package_truncated() {
        use crate::package_cache::TestPackageCache;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let mut cache = TestPackageCache::default();
        cache
            .versions
            .insert("mail-transport-agent".to_string(), Vec::new());
        cache.providers.insert(
            "mail-transport-agent".to_string(),
            vec![
                "courier-mta".to_string(),
                "exim4-daemon-heavy".to_string(),
                "exim4-daemon-light".to_string(),
                "postfix".to_string(),
                "sendmail-bin".to_string(),
            ],
        );
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));

        let content = "\
Source: test-package

Package: test-package
Depends: mail-transport-agent
Description: A test
";
        let parsed = debian_control::lossless::Control::parse(content);
        let range = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position::new(0, 0),
            end: tower_lsp_server::ls_types::Position::new(5, 0),
        };

        let (hints, _uncached) =
            generate_inlay_hints(&parsed, content, &range, &shared_cache, &HashMap::new()).await;

        assert_eq!(hints.len(), 1);
        match &hints[0].label {
            InlayHintLabel::String(s) => {
                assert_eq!(
                    s,
                    "[-> courier-mta | exim4-daemon-heavy | exim4-daemon-light | ...]"
                )
            }
            _ => panic!("Expected string label"),
        }
    }

    #[tokio::test]
    async fn test_inlay_hint_archive_version() {
        use crate::package_cache::{TestPackageCache, VersionInfo};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let mut cache = TestPackageCache::default();
        cache
            .packages
            .push(("python3-all".to_string(), Some("Python 3".to_string())));
        cache.versions.insert(
            "python3-all".to_string(),
            vec![VersionInfo {
                version: "3.12.8-1".to_string(),
                suites: vec!["unstable".to_string()],
            }],
        );
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));

        let content = "\
Source: test-package

Package: test-package
Depends: python3-all
Description: A test
";
        let parsed = debian_control::lossless::Control::parse(content);
        let range = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position::new(0, 0),
            end: tower_lsp_server::ls_types::Position::new(5, 0),
        };

        let (hints, _uncached) =
            generate_inlay_hints(&parsed, content, &range, &shared_cache, &HashMap::new()).await;

        assert_eq!(hints.len(), 1);
        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "[unstable: 3.12.8-1]"),
            _ => panic!("Expected string label"),
        }
    }

    #[tokio::test]
    async fn test_inlay_hint_archive_version_not_shown_without_cache() {
        use crate::package_cache::TestPackageCache;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let mut cache = TestPackageCache::default();
        // Package is known but versions are not cached yet
        cache
            .packages
            .push(("python3-all".to_string(), Some("Python 3".to_string())));
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));

        let content = "\
Source: test-package

Package: test-package
Depends: python3-all
Description: A test
";
        let parsed = debian_control::lossless::Control::parse(content);
        let range = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position::new(0, 0),
            end: tower_lsp_server::ls_types::Position::new(5, 0),
        };

        let (hints, _uncached) =
            generate_inlay_hints(&parsed, content, &range, &shared_cache, &HashMap::new()).await;

        // No hint yet — versions will be loaded in background
        assert_eq!(hints.len(), 0);
    }

    #[tokio::test]
    async fn test_inlay_hint_archive_version_multiple_suites() {
        use crate::package_cache::{TestPackageCache, VersionInfo};
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let mut cache = TestPackageCache::default();
        cache
            .packages
            .push(("debhelper".to_string(), Some("helper".to_string())));
        cache.versions.insert(
            "debhelper".to_string(),
            vec![
                VersionInfo {
                    version: "13.31".to_string(),
                    suites: vec!["unstable".to_string(), "testing".to_string()],
                },
                VersionInfo {
                    version: "13.3.4".to_string(),
                    suites: vec!["bullseye".to_string()],
                },
            ],
        );
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));

        let content = "\
Source: test-package

Package: test-package
Depends: debhelper
Description: A test
";
        let parsed = debian_control::lossless::Control::parse(content);
        let range = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position::new(0, 0),
            end: tower_lsp_server::ls_types::Position::new(5, 0),
        };

        let (hints, _uncached) =
            generate_inlay_hints(&parsed, content, &range, &shared_cache, &HashMap::new()).await;

        assert_eq!(hints.len(), 1);
        match &hints[0].label {
            InlayHintLabel::String(s) => {
                assert_eq!(s, "[unstable,testing: 13.31 | bullseye: 13.3.4]")
            }
            _ => panic!("Expected string label"),
        }
    }

    #[tokio::test]
    async fn test_inlay_hint_substvar() {
        use crate::package_cache::TestPackageCache;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let cache = TestPackageCache::default();
        let shared_cache: crate::package_cache::SharedPackageCache = Arc::new(RwLock::new(cache));

        let mut resolved = HashMap::new();
        resolved.insert(
            "shlibs:Depends".to_string(),
            "libc6 (>= 2.17), libfoo1 (>= 1.0)".to_string(),
        );

        let content = "\
Source: test-package

Package: test-package
Depends: ${shlibs:Depends}, ${misc:Depends}
Description: A test
";
        let parsed = debian_control::lossless::Control::parse(content);
        let range = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position::new(0, 0),
            end: tower_lsp_server::ls_types::Position::new(5, 0),
        };

        let (hints, _uncached) =
            generate_inlay_hints(&parsed, content, &range, &shared_cache, &resolved).await;

        // Should have hint for ${shlibs:Depends} only (not ${misc:Depends})
        let sv_hints: Vec<_> = hints
            .iter()
            .filter(|h| matches!(&h.label, InlayHintLabel::String(s) if s.starts_with("[= ")))
            .collect();
        assert_eq!(sv_hints.len(), 1);
        match &sv_hints[0].label {
            InlayHintLabel::String(s) => {
                assert_eq!(s, "[= libc6 (>= 2.17), libfoo1 (>= 1.0)]")
            }
            _ => panic!("Expected string label"),
        }
    }
}
