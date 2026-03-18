//! Inlay hints for debian/control files.
//!
//! Shows whether the Standards-Version is current or outdated:
//!   Standards-Version: 4.6.2   [latest: 4.7.0]

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
    let Ok(control) = parsed.clone().to_result() else {
        return Vec::new();
    };

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

/// Generate inlay hints for the Standards-Version field in a control file.
///
/// If the Standards-Version is outdated compared to the latest debian-policy
/// version available in the package cache, an inlay hint is shown after the
/// value: `[latest: X.Y.Z]`.
pub async fn generate_inlay_hints(
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    source_text: &str,
    range: &tower_lsp_server::ls_types::Range,
    package_cache: &crate::package_cache::SharedPackageCache,
) -> Vec<InlayHint> {
    // Extract Standards-Version info synchronously (CST types are not Send)
    let sv_entries = find_standards_versions(parsed, source_text, range);
    if sv_entries.is_empty() {
        return Vec::new();
    }

    // Look up latest debian-policy version from the package cache (async)
    let latest_standards = {
        let mut cache = package_cache.write().await;
        let versions = cache.load_versions("debian-policy").await;
        versions.and_then(|vs| {
            vs.first()
                .and_then(|v| policy_version_to_standards_version(&v.version))
                .map(|s| s.to_string())
        })
    };

    let Some(latest) = latest_standards else {
        return Vec::new();
    };

    let mut hints = Vec::new();

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

    hints
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

        let hints = generate_inlay_hints(&parsed, content, &range, &shared_cache).await;

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

        let hints = generate_inlay_hints(&parsed, content, &range, &shared_cache).await;

        assert_eq!(hints.len(), 0);
    }
}
