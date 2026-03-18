//! Inlay hints for debian/changelog files.
//!
//! Shows distribution-to-suite mappings as inlay hints:
//! - When distribution is UNRELEASED, shows the target distribution from the previous entry
//! - When distribution is an alias (e.g. "unstable"), shows the codename (e.g. "sid")
//! - When distribution is a codename (e.g. "trixie"), shows the alias (e.g. "testing")

use distro_info::{DebianDistroInfo, DistroInfo};
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{InlayHint, InlayHintKind, InlayHintLabel};

use crate::position::text_range_to_lsp_range;

/// Resolve the current testing, stable, and oldstable codenames from distro-info.
///
/// Returns `(testing, stable, oldstable)` where each is `Option<String>`.
fn resolve_suite_codenames(
    debian_info: &DebianDistroInfo,
) -> (Option<String>, Option<String>, Option<String>) {
    let today = chrono::Local::now().date_naive();

    // "released" releases that are still supported, sorted by release date.
    // These have actual version numbers and release dates.
    // Filter out sid and experimental which have no version.
    let supported = debian_info.supported(today);
    let mut released_supported: Vec<_> = supported
        .iter()
        .filter(|r| r.version().is_some() && r.release().is_some())
        .collect();
    released_supported.sort_by_key(|r| r.release());

    // stable = most recently released supported release
    let stable = released_supported.last().map(|r| r.series().to_string());

    // oldstable = second most recently released supported release
    let oldstable = if released_supported.len() >= 2 {
        Some(
            released_supported[released_supported.len() - 2]
                .series()
                .to_string(),
        )
    } else {
        None
    };

    // testing = has a version number but no release date yet (not sid/experimental)
    let testing = debian_info
        .iter()
        .find(|r| r.version().is_some() && r.release().is_none())
        .map(|r| r.series().to_string());

    (testing, stable, oldstable)
}

/// Map a distribution alias to its codename or vice versa.
///
/// Returns `None` if there is no mapping (e.g. the distribution is already
/// unambiguous, or we can't load distro-info data).
fn get_distribution_mapping(distribution: &str) -> Option<String> {
    let Ok(debian_info) = DebianDistroInfo::new() else {
        // Fallback: we know "unstable" is always "sid"
        return match distribution {
            "unstable" => Some("sid".to_string()),
            "sid" => Some("unstable".to_string()),
            _ => None,
        };
    };

    let (testing, stable, oldstable) = resolve_suite_codenames(&debian_info);

    match distribution {
        "unstable" => Some("sid".to_string()),
        "sid" => Some("unstable".to_string()),
        "testing" => testing,
        "stable" => stable,
        "oldstable" => oldstable,
        "experimental" => None,
        codename => {
            if testing.as_deref() == Some(codename) {
                return Some("testing".to_string());
            }
            if stable.as_deref() == Some(codename) {
                return Some("stable".to_string());
            }
            if oldstable.as_deref() == Some(codename) {
                return Some("oldstable".to_string());
            }
            None
        }
    }
}

/// Generate inlay hints for changelog distribution fields.
pub fn generate_inlay_hints(
    parsed: &debian_changelog::Parse<debian_changelog::ChangeLog>,
    source_text: &str,
    range: &tower_lsp_server::ls_types::Range,
) -> Vec<InlayHint> {
    let changelog = parsed.tree();
    let mut hints = Vec::new();

    let target_distribution = super::get_target_distribution(&changelog);

    let text_range = match crate::position::try_lsp_range_to_text_range(source_text, range) {
        Some(r) => r,
        None => return hints,
    };

    for entry in changelog.entries_in_range(text_range) {
        let Some(dists) = entry.distributions() else {
            continue;
        };
        if dists.is_empty() {
            continue;
        }

        let dist = &dists[0];

        let hint_text = if dist == "UNRELEASED" {
            Some(format!("-> {}", target_distribution))
        } else {
            get_distribution_mapping(dist).map(|mapped| format!("= {}", mapped))
        };

        let Some(hint_text) = hint_text else {
            continue;
        };

        // Find the position of the distribution text in the entry
        let entry_text = entry.syntax().text().to_string();
        // The distribution appears after ") " in the header
        let Some(close_paren_offset) = entry_text.find(") ") else {
            continue;
        };
        let dist_start = close_paren_offset + 2;
        let dist_end = dist_start + dist.len();

        let entry_range = entry.syntax().text_range();
        let abs_end = entry_range.start() + text_size::TextSize::from(dist_end as u32);

        let lsp_range =
            text_range_to_lsp_range(source_text, text_size::TextRange::new(abs_end, abs_end));

        hints.push(InlayHint {
            position: lsp_range.start,
            label: InlayHintLabel::String(hint_text),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unstable_maps_to_sid() {
        assert_eq!(
            get_distribution_mapping("unstable"),
            Some("sid".to_string())
        );
    }

    #[test]
    fn test_sid_maps_to_unstable() {
        assert_eq!(
            get_distribution_mapping("sid"),
            Some("unstable".to_string())
        );
    }

    #[test]
    fn test_experimental_has_no_mapping() {
        assert_eq!(get_distribution_mapping("experimental"), None);
    }

    #[test]
    fn test_testing_maps_to_codename() {
        // testing should map to a codename, not sid or experimental
        let result = get_distribution_mapping("testing");
        if let Some(ref codename) = result {
            assert_ne!(codename, "sid");
            assert_ne!(codename, "experimental");
        }
    }

    #[test]
    fn test_stable_maps_to_codename() {
        // stable should map to a released codename
        let result = get_distribution_mapping("stable");
        if let Some(ref codename) = result {
            assert_ne!(codename, "sid");
            assert_ne!(codename, "experimental");
            assert_ne!(codename, "unstable");
        }
    }

    #[test]
    fn test_resolve_suite_codenames() {
        let Ok(debian_info) = DebianDistroInfo::new() else {
            return; // Skip if distro-info not available
        };
        let (testing, stable, oldstable) = resolve_suite_codenames(&debian_info);

        // All resolved codenames (if present) should be distinct
        let mut seen = std::collections::HashSet::new();
        for name in [&testing, &stable, &oldstable].into_iter().flatten() {
            assert!(seen.insert(name.clone()), "Duplicate codename: {}", name);
        }

        // None should be sid or experimental
        for name in [&testing, &stable, &oldstable].into_iter().flatten() {
            assert_ne!(name, "sid");
            assert_ne!(name, "experimental");
        }
    }

    #[test]
    fn test_inlay_hint_for_unreleased() {
        let changelog_text = r#"foo (1.0-2) UNRELEASED; urgency=medium

  * New changes.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000

foo (1.0-1) unstable; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;

        let parsed = debian_changelog::ChangeLog::parse(changelog_text);
        let range = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position::new(0, 0),
            end: tower_lsp_server::ls_types::Position::new(11, 0),
        };

        let hints = generate_inlay_hints(&parsed, changelog_text, &range);

        // Should have hints for both UNRELEASED (-> unstable) and unstable (= sid)
        assert_eq!(hints.len(), 2);

        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "-> unstable"),
            _ => panic!("Expected string label"),
        }

        match &hints[1].label {
            InlayHintLabel::String(s) => assert_eq!(s, "= sid"),
            _ => panic!("Expected string label"),
        }
    }

    #[test]
    fn test_inlay_hint_for_unstable_only() {
        let changelog_text = r#"foo (1.0-1) unstable; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;

        let parsed = debian_changelog::ChangeLog::parse(changelog_text);
        let range = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position::new(0, 0),
            end: tower_lsp_server::ls_types::Position::new(4, 0),
        };

        let hints = generate_inlay_hints(&parsed, changelog_text, &range);

        assert_eq!(hints.len(), 1);

        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "= sid"),
            _ => panic!("Expected string label"),
        }
    }

    #[test]
    fn test_no_inlay_hint_for_experimental() {
        let changelog_text = r#"foo (1.0-1) experimental; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;

        let parsed = debian_changelog::ChangeLog::parse(changelog_text);
        let range = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position::new(0, 0),
            end: tower_lsp_server::ls_types::Position::new(4, 0),
        };

        let hints = generate_inlay_hints(&parsed, changelog_text, &range);

        assert_eq!(hints.len(), 0);
    }
}
