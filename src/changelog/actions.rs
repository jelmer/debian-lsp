use chrono::Local;
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::TextEdit;

use crate::position::text_range_to_lsp_range;

/// Generate a TextEdit that updates the timestamp of the first UNRELEASED entry
/// to the current time. Returns `None` if the first entry is not UNRELEASED or
/// has no timestamp.
pub fn generate_timestamp_update_edit(
    changelog: &debian_changelog::ChangeLog,
    source_text: &str,
) -> Option<TextEdit> {
    let entry = changelog.iter().next()?;

    // Only update UNRELEASED entries
    let dists = entry.distributions()?;
    if dists.is_empty() || dists[0] != "UNRELEASED" {
        return None;
    }

    // Find the Timestamp node in the entry footer
    let footer = entry.syntax().children().find_map(|n| {
        if n.kind() == debian_changelog::SyntaxKind::ENTRY_FOOTER {
            Some(n)
        } else {
            None
        }
    })?;

    let timestamp_node = footer
        .children()
        .find(|n| n.kind() == debian_changelog::SyntaxKind::TIMESTAMP)?;

    let old_timestamp = timestamp_node.text().to_string();
    let new_timestamp = Local::now().format("%a, %d %b %Y %H:%M:%S %z").to_string();

    // Don't generate an edit if the timestamp hasn't changed (same second)
    if old_timestamp.trim() == new_timestamp.trim() {
        return None;
    }

    let range = text_range_to_lsp_range(source_text, timestamp_node.text_range());

    Some(TextEdit {
        range,
        new_text: new_timestamp,
    })
}

/// Determine the appropriate distribution to use when marking an entry for upload
pub fn get_target_distribution(changelog: &debian_changelog::ChangeLog) -> String {
    // Look for the most recent released entry (not UNRELEASED)
    changelog
        .iter()
        .skip(1) // Skip the first entry
        .find_map(|entry| {
            entry.distributions().and_then(|dists| {
                if !dists.is_empty() && dists[0] != "UNRELEASED" {
                    Some(dists[0].clone())
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| "unstable".to_string())
}

/// Generates a new changelog entry text with incremented debian revision
pub fn generate_new_changelog_entry(
    current_changelog: &debian_changelog::ChangeLog,
) -> Result<String, String> {
    let mut changelog = current_changelog.clone();
    let entry = changelog
        .new_entry()
        .urgency(debian_changelog::Urgency::Medium)
        .change_line("* ".to_string())
        .finish();

    Ok(format!("{}\n", entry))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_generate_timestamp_update_edit_unreleased() {
        let changelog_text = r#"foo (1.0-2) UNRELEASED; urgency=medium

  * New changes.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;

        let parsed = debian_changelog::ChangeLog::parse(changelog_text);
        let changelog = parsed.tree();

        let edit = generate_timestamp_update_edit(&changelog, changelog_text);
        assert!(
            edit.is_some(),
            "should generate an edit for UNRELEASED entry"
        );

        let edit = edit.unwrap();
        assert!(
            edit.new_text.contains(", "),
            "new timestamp should be a valid date: {}",
            edit.new_text
        );
        // Verify the range points to the timestamp on line 4
        assert_eq!(edit.range.start.line, 4);
    }

    #[test]
    fn test_generate_timestamp_update_edit_released() {
        let changelog_text = r#"foo (1.0-1) unstable; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;

        let parsed = debian_changelog::ChangeLog::parse(changelog_text);
        let changelog = parsed.tree();

        let edit = generate_timestamp_update_edit(&changelog, changelog_text);
        assert!(
            edit.is_none(),
            "should not generate an edit for released entry"
        );
    }

    #[test]
    fn test_generate_new_changelog_entry() {
        let changelog_text = r#"foo (1.0-1) unstable; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;

        let parsed = debian_changelog::ChangeLog::parse(changelog_text);
        let changelog = parsed.tree();

        // Set environment variables for predictable maintainer info
        env::set_var("DEBFULLNAME", "Test User");
        env::set_var("DEBEMAIL", "test@example.com");

        let new_entry = generate_new_changelog_entry(&changelog).unwrap();

        // Parse the generated entry to verify structure
        let lines: Vec<&str> = new_entry.lines().collect();

        // Check the header line has correct package and version with UNRELEASED
        assert_eq!(lines[0], "foo (1.0-2) UNRELEASED; urgency=medium");

        // Check empty line after header
        assert_eq!(lines[1], "");

        // Check bullet point line
        assert_eq!(lines[2], "  * ");

        // Check empty line before signature
        assert_eq!(lines[3], "");

        // Check signature line starts correctly
        assert!(lines[4].starts_with(" -- Test User <test@example.com>  "));

        // Clean up environment
        env::remove_var("DEBFULLNAME");
        env::remove_var("DEBEMAIL");
    }

    #[test]
    fn test_get_target_distribution_from_previous_entry() {
        let changelog_text = r#"foo (1.0-2) UNRELEASED; urgency=medium

  * New changes.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000

foo (1.0-1) unstable; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;

        let parsed = debian_changelog::ChangeLog::parse(changelog_text);
        let changelog = parsed.tree();

        let target = get_target_distribution(&changelog);
        assert_eq!(target, "unstable");
    }

    #[test]
    fn test_get_target_distribution_defaults_to_unstable() {
        let changelog_text = r#"foo (1.0-1) UNRELEASED; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;

        let parsed = debian_changelog::ChangeLog::parse(changelog_text);
        let changelog = parsed.tree();

        let target = get_target_distribution(&changelog);
        assert_eq!(target, "unstable");
    }

    #[test]
    fn test_get_target_distribution_skips_unreleased_entries() {
        let changelog_text = r#"foo (1.0-3) UNRELEASED; urgency=medium

  * Latest changes.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000

foo (1.0-2) UNRELEASED; urgency=medium

  * More changes.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000

foo (1.0-1) experimental; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;

        let parsed = debian_changelog::ChangeLog::parse(changelog_text);
        let changelog = parsed.tree();

        let target = get_target_distribution(&changelog);
        assert_eq!(target, "experimental");
    }
}
