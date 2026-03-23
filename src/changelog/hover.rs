//! Hover information for debian/changelog files.
//!
//! Shows bug details when hovering over `Closes: #NNN` or `LP: #NNN`
//! references in changelog detail lines.  When a UDD/Launchpad connection
//! is available the hover includes title, severity/status and other metadata;
//! otherwise a plain link to the bug tracker is shown.

use debian_changelog::bugs::Bug;
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};

use crate::bugs::{DebbugsBugSummary, LaunchpadBugSummary, SharedBugCache};

/// Get hover information for a bug reference in a changelog file.
///
/// Fetches bug details from the cache (populating it from UDD/Launchpad on
/// first access for the package).  Returns `None` when the cursor is not on a
/// bug reference.
pub async fn get_hover(
    parse: &debian_changelog::Parse<debian_changelog::ChangeLog>,
    source_text: &str,
    position: Position,
    bug_cache: &SharedBugCache,
) -> Option<Hover> {
    // Extract bug ref in a non-Send scope, then drop all CST values before
    // the first await so the future remains Send.
    let bug = {
        let changelog = debian_changelog::ChangeLog::cast(parse.syntax_node())?;
        let offset = crate::position::try_position_to_offset(source_text, position)?;
        let entry = changelog.entry_at_offset(offset)?;
        entry.bug_at_offset(offset)?
    };

    match &bug {
        Bug::Debian(id) => {
            let summary = bug_cache.write().await.get_debian_bug_summary(*id).await;
            Some(match summary {
                Some(s) => make_debian_hover(&s),
                None => make_fallback_hover(&bug),
            })
        }
        Bug::Launchpad(id) => {
            let summary = bug_cache.write().await.get_launchpad_bug_summary(*id).await;
            Some(match summary {
                Some(s) => make_launchpad_hover(&s),
                None => make_fallback_hover(&bug),
            })
        }
    }
}

/// Minimal hover shown when bug details are not available.
fn make_fallback_hover(bug: &Bug) -> Hover {
    let label = match bug {
        Bug::Debian(id) => format!("Debian Bug #{}", id),
        Bug::Launchpad(id) => format!("Launchpad Bug #{}", id),
    };

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("**[{}]({})** ", label, bug.url()),
        }),
        range: None,
    }
}

fn make_debian_hover(summary: &DebbugsBugSummary) -> Hover {
    let title = summary.title.as_deref().unwrap_or("(no title)");
    let mut lines = vec![format!(
        "**[Debian Bug #{}](https://bugs.debian.org/{})** — {}",
        summary.id, summary.id, title
    )];

    if let Some(severity) = &summary.severity {
        lines.push(format!("**Severity:** {}", severity));
    }
    if summary.done {
        lines.push("**Status:** done".to_string());
    } else {
        lines.push("**Status:** open".to_string());
    }
    if let Some(originator) = &summary.originator {
        if !originator.is_empty() {
            lines.push(format!("**Reported by:** {}", originator));
        }
    }
    if let Some(tags) = &summary.tags {
        if !tags.is_empty() {
            lines.push(format!("**Tags:** {}", tags));
        }
    }
    if let Some(forwarded) = &summary.forwarded {
        if !forwarded.is_empty() {
            lines.push(format!("**Forwarded:** {}", forwarded));
        }
    }

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n\n"),
        }),
        range: None,
    }
}

fn make_launchpad_hover(summary: &LaunchpadBugSummary) -> Hover {
    let title = summary.title.as_deref().unwrap_or("(no title)");
    let mut lines = vec![format!(
        "**[Launchpad Bug #{}](https://bugs.launchpad.net/bugs/{})** — {}",
        summary.id, summary.id, title
    )];

    if let Some(status) = &summary.status {
        lines.push(format!("**Status:** {}", status));
    }
    lines.push(if summary.done {
        "**Completion:** complete".to_string()
    } else {
        "**Completion:** open".to_string()
    });

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n\n"),
        }),
        range: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use text_size::TextSize;

    fn parse_for(text: &str) -> debian_changelog::Parse<debian_changelog::ChangeLog> {
        debian_changelog::ChangeLog::parse(text)
    }

    /// Helper: detect the bug ref without going through the async path.
    fn find_bug_ref(text: &str, byte_offset: usize) -> Option<Bug> {
        let parsed = parse_for(text);
        let changelog = debian_changelog::ChangeLog::cast(parsed.syntax_node())?;
        let offset = TextSize::try_from(byte_offset).ok()?;
        let entry = changelog.entry_at_offset(offset)?;
        entry.bug_at_offset(offset)
    }

    #[test]
    fn test_detect_closes_bug() {
        let text = "foo (1.0-1) unstable; urgency=medium\n\n  * Fixed a bug. (Closes: #123456)\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let offset = text.find("#123456").unwrap() + 1;
        assert_eq!(find_bug_ref(text, offset), Some(Bug::Debian(123456)));
    }

    #[test]
    fn test_detect_lp_bug() {
        let text = "foo (1.0-1) unstable; urgency=medium\n\n  * Fixed a bug. (LP: #987654)\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let offset = text.find("#987654").unwrap() + 1;
        assert_eq!(find_bug_ref(text, offset), Some(Bug::Launchpad(987654)));
    }

    #[test]
    fn test_detect_multiple_closes_first() {
        let text = "foo (1.0-1) unstable; urgency=medium\n\n  * Fixed bugs. (Closes: #111, #222)\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let offset = text.find("#111").unwrap() + 1;
        assert_eq!(find_bug_ref(text, offset), Some(Bug::Debian(111)));
    }

    #[test]
    fn test_detect_multiple_closes_second() {
        let text = "foo (1.0-1) unstable; urgency=medium\n\n  * Fixed bugs. (Closes: #111, #222)\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let offset = text.find("#222").unwrap() + 1;
        assert_eq!(find_bug_ref(text, offset), Some(Bug::Debian(222)));
    }

    #[test]
    fn test_no_bug_on_regular_text() {
        let text = "foo (1.0-1) unstable; urgency=medium\n\n  * Just a regular change.\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let offset = text.find("regular").unwrap();
        assert_eq!(find_bug_ref(text, offset), None);
    }

    #[test]
    fn test_no_bug_on_header() {
        let text = "foo (1.0-1) unstable; urgency=medium\n\n  * Fixed. (Closes: #123)\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        assert_eq!(find_bug_ref(text, 0), None);
    }

    /// Extract the markdown value from a Hover, panicking if not markup.
    fn hover_markdown(hover: &Hover) -> &str {
        match &hover.contents {
            HoverContents::Markup(m) => {
                assert_eq!(m.kind, MarkupKind::Markdown);
                &m.value
            }
            other => panic!("Expected markup content, got {:?}", other),
        }
    }

    #[test]
    fn test_make_debian_hover_with_details() {
        let summary = DebbugsBugSummary {
            id: 123456,
            title: Some("FTBFS with GCC 14".to_string()),
            severity: Some("serious".to_string()),
            done: false,
            tags: Some("patch".to_string()),
            forwarded: None,
            originator: Some("someone@example.com".to_string()),
        };
        let hover = make_debian_hover(&summary);
        assert_eq!(
            hover_markdown(&hover),
            "**[Debian Bug #123456](https://bugs.debian.org/123456)** — FTBFS with GCC 14\n\
             \n\
             **Severity:** serious\n\
             \n\
             **Status:** open\n\
             \n\
             **Reported by:** someone@example.com\n\
             \n\
             **Tags:** patch"
        );
    }

    #[test]
    fn test_make_debian_hover_done() {
        let summary = DebbugsBugSummary {
            id: 1,
            title: Some("Fixed".to_string()),
            severity: None,
            done: true,
            tags: None,
            forwarded: None,
            originator: None,
        };
        let hover = make_debian_hover(&summary);
        assert_eq!(
            hover_markdown(&hover),
            "**[Debian Bug #1](https://bugs.debian.org/1)** — Fixed\n\
             \n\
             **Status:** done"
        );
    }

    #[test]
    fn test_make_launchpad_hover_with_details() {
        let summary = LaunchpadBugSummary {
            id: 987654,
            title: Some("Crash on startup".to_string()),
            status: Some("Confirmed".to_string()),
            done: false,
        };
        let hover = make_launchpad_hover(&summary);
        assert_eq!(
            hover_markdown(&hover),
            "**[Launchpad Bug #987654](https://bugs.launchpad.net/bugs/987654)** — Crash on startup\n\
             \n\
             **Status:** Confirmed\n\
             \n\
             **Completion:** open"
        );
    }

    #[test]
    fn test_fallback_hover_debian() {
        let hover = make_fallback_hover(&Bug::Debian(42));
        assert_eq!(
            hover_markdown(&hover),
            "**[Debian Bug #42](https://bugs.debian.org/42)** "
        );
    }

    #[test]
    fn test_fallback_hover_launchpad() {
        let hover = make_fallback_hover(&Bug::Launchpad(42));
        assert_eq!(
            hover_markdown(&hover),
            "**[Launchpad Bug #42](https://bugs.launchpad.net/bugs/42)** "
        );
    }
}
