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

/// Create an inlay hint at the given source offset with the given label.
///
/// Converts the offset to an LSP position and constructs the hint with
/// standard padding and kind settings.
fn make_hint(source_text: &str, offset: TextSize, label: String) -> InlayHint {
    let lsp_range = text_range_to_lsp_range(source_text, text_size::TextRange::new(offset, offset));
    InlayHint {
        position: lsp_range.start,
        label: InlayHintLabel::String(label),
        kind: Some(InlayHintKind::TYPE),
        text_edits: None,
        tooltip: None,
        padding_left: Some(true),
        padding_right: None,
        data: None,
    }
}

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

/// Info about a relation in a dependency field.
struct RelationInfo {
    /// The package name.
    name: String,
    /// The end position of the relation in the source text.
    relation_end: TextSize,
    /// Whether this is a `debhelper-compat (= N)` relation.
    is_debhelper_compat: bool,
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

                    let is_debhelper_compat = name == "debhelper-compat"
                        && matches!(relation.version(), Some((VersionConstraint::Equal, _)));

                    rel_results.push(RelationInfo {
                        name,
                        relation_end: absolute_end,
                        is_debhelper_compat,
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

/// Format version info as a compact string without brackets.
///
/// Examples:
///   `"sid,trixie: 13.31 | bullseye: 13.3.4"`
///   `"available: 13.31"` (no suite info)
fn format_version_info(versions: &[crate::package_cache::VersionInfo]) -> Option<String> {
    if versions.is_empty() {
        return None;
    }

    let has_suites = versions.iter().any(|v| !v.suites.is_empty());

    if !has_suites {
        return Some(format!("available: {}", versions[0].version));
    }

    let parts: Vec<String> = versions
        .iter()
        .filter(|v| !v.suites.is_empty())
        .map(|v| format!("{}: {}", v.suites.join(","), v.version))
        .collect();

    if parts.is_empty() {
        return Some(format!("available: {}", versions[0].version));
    }

    Some(parts.join(" | "))
}

/// Format a compact version hint from cached version info, wrapped in brackets.
///
/// Examples:
///   `[sid,trixie: 13.31 | bullseye: 13.3.4]`
///   `[available: 13.31]` (no suite info)
fn format_version_hint(versions: &[crate::package_cache::VersionInfo]) -> Option<String> {
    format_version_info(versions).map(|s| format!("[{}]", s))
}

/// Format a version string for a single package, showing just the candidate
/// (first/newest) version without suite detail.
fn format_short_version(versions: &[crate::package_cache::VersionInfo]) -> Option<String> {
    versions.first().map(|v| v.version.clone())
}

/// Format a compact provider hint for a virtual package, including version
/// info for each provider when cached.
///
/// Strategy for keeping the hint within `max_len` characters:
/// 1. Try full per-suite version info for each provider
/// 2. If too long, fall back to just the candidate version
/// 3. If still too long, truncate the provider list with `...`
fn format_provider_hint(
    providers: &[String],
    cache: &dyn crate::package_cache::PackageCache,
    uncached: &mut Vec<String>,
    max_len: usize,
) -> String {
    // Annotate each provider with its version info
    let annotated: Vec<(String, Option<String>, Option<String>)> = providers
        .iter()
        .map(|p| {
            if let Some(versions) = cache.get_cached_versions(p) {
                let full = format_version_info(versions);
                let short = format_short_version(versions);
                (p.clone(), full, short)
            } else {
                uncached.push(p.clone());
                (p.clone(), None, None)
            }
        })
        .collect();

    // Try 1: full suite detail for all providers
    let full_parts: Vec<String> = annotated
        .iter()
        .map(|(name, full, _)| match full {
            Some(v) => format!("{} ({})", name, v),
            None => name.clone(),
        })
        .collect();
    let candidate = format!("[-> {}]", full_parts.join(" | "));
    if candidate.len() <= max_len {
        return candidate;
    }

    // Try 2: just candidate version for all providers
    let short_parts: Vec<String> = annotated
        .iter()
        .map(|(name, _, short)| match short {
            Some(v) => format!("{} ({})", name, v),
            None => name.clone(),
        })
        .collect();
    let candidate = format!("[-> {}]", short_parts.join(" | "));
    if candidate.len() <= max_len {
        return candidate;
    }

    // Try 3: truncate providers until it fits
    for n in (1..short_parts.len()).rev() {
        let candidate = format!("[-> {} | ...]", short_parts[..n].join(" | "));
        if candidate.len() <= max_len {
            return candidate;
        }
    }

    // Last resort: just the first provider
    format!("[-> {} | ...]", short_parts[0])
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
    let (rel_entries, substvar_entries) = find_relations(parsed, source_text, range);

    let has_debhelper_compat = rel_entries.iter().any(|r| r.is_debhelper_compat);

    if sv_entries.is_empty() && rel_entries.is_empty() && substvar_entries.is_empty() {
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
                hints.push(make_hint(
                    source_text,
                    sv.value_end,
                    format!("[latest: {}]", latest),
                ));
            }
        }
    }

    // debhelper-compat hints
    let latest_compat = if has_debhelper_compat {
        let mut cache = package_cache.write().await;
        let versions = cache.load_versions("debhelper").await;
        versions.and_then(|vs| {
            vs.first()
                .and_then(|v| debhelper_version_to_compat_level(&v.version))
        })
    } else {
        None
    };

    // Per-relation hints (archive versions, virtual package providers,
    // debhelper-compat). Use only cached data (read lock) for archive
    // lookups to avoid blocking the LSP response. Returns uncached package
    // names so the caller can trigger background loading and an
    // inlayHint/refresh.
    let mut uncached_packages = Vec::new();
    {
        let cache = package_cache.read().await;
        for rel in &rel_entries {
            if rel.is_debhelper_compat {
                if let Some(latest) = latest_compat {
                    hints.push(make_hint(
                        source_text,
                        rel.relation_end,
                        format!("[current: {}]", latest),
                    ));
                }
                continue;
            }

            let cached_versions = cache.get_cached_versions(&rel.name);
            let cached_providers = cache.get_cached_providers(&rel.name);

            if let Some(versions) = cached_versions {
                if !versions.is_empty() {
                    // Real package with version info — show archive versions
                    if let Some(label) = format_version_hint(versions) {
                        hints.push(make_hint(source_text, rel.relation_end, label));
                    }
                } else if let Some(providers) = cached_providers {
                    // Versions cached but empty = virtual package; show providers
                    // with their available versions
                    if !providers.is_empty() {
                        let label =
                            format_provider_hint(providers, &*cache, &mut uncached_packages, 80);
                        hints.push(make_hint(source_text, rel.relation_end, label));
                    }
                }
                // else: versions empty, no providers cached — will be loaded in background
            } else {
                // Versions not cached yet
                uncached_packages.push(rel.name.clone());
            }
        }
    }

    // Deduplicate uncached packages since the same package can appear in
    // multiple dependency fields.
    uncached_packages.sort();
    uncached_packages.dedup();

    // Substvar hints
    for sv in &substvar_entries {
        if let Some(value) = resolved_substvars.get(&sv.name) {
            hints.push(make_hint(
                source_text,
                sv.substvar_end,
                format!("[= {}]", value),
            ));
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

        // Expect debhelper version hint + debhelper-compat hint
        // (debhelper appears before debhelper-compat in the relations)
        assert_eq!(hints.len(), 2);
        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "[unstable: 14.2]"),
            _ => panic!("Expected string label"),
        }
        assert_eq!(hints[0].position.line, 1);
        match &hints[1].label {
            InlayHintLabel::String(s) => assert_eq!(s, "[current: 14]"),
            _ => panic!("Expected string label"),
        }
        // The compat hint should be on line 2 (the debhelper-compat line)
        assert_eq!(hints[1].position.line, 2);
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
                    "[-> courier-mta | exim4-daemon-heavy | exim4-daemon-light | postfix | ...]"
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
