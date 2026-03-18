//! Inlay hints for debian/changelog files.
//!
//! Shows distribution-to-suite mappings as inlay hints:
//! - When distribution is UNRELEASED, shows the target distribution from the previous entry
//! - When distribution is an alias (e.g. "unstable"), shows the codename (e.g. "sid")
//! - When distribution is a codename (e.g. "trixie"), shows the alias (e.g. "testing")
//!
//! Suite resolution is date-aware: an entry from 2020 with "stable" will
//! resolve to "buster", not the current stable release.

use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{InlayHint, InlayHintKind, InlayHintLabel};

use crate::position::text_range_to_lsp_range;

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
            // Use the entry's timestamp for date-aware suite resolution,
            // falling back to today if the timestamp can't be parsed.
            let date = entry
                .datetime()
                .map(|dt| dt.date_naive())
                .unwrap_or_else(|| chrono::Local::now().date_naive());
            crate::distros::get_distribution_mapping_at(dist, date)
                .map(|mapped| format!("= {}", mapped))
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

    #[test]
    fn test_stable_resolves_to_date_of_entry() {
        if !crate::distros::has_distro_info() {
            return; // distro-info-data not available (e.g. Windows)
        }
        // An entry from 2020 with "stable" should resolve to "buster"
        let changelog_text = r#"foo (1.0-1) stable; urgency=medium

  * Stable update.

 -- John Doe <john@example.com>  Mon, 01 Jun 2020 12:00:00 +0000
"#;

        let parsed = debian_changelog::ChangeLog::parse(changelog_text);
        let range = tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position::new(0, 0),
            end: tower_lsp_server::ls_types::Position::new(4, 0),
        };

        let hints = generate_inlay_hints(&parsed, changelog_text, &range);

        assert_eq!(hints.len(), 1);

        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "= buster"),
            _ => panic!("Expected string label"),
        }
    }
}
