//! Hover information for debian/changelog files.
//!
//! Shows bug details when hovering over `Closes: #NNN` or `LP: #NNN`
//! references in changelog detail lines.  When a UDD/Launchpad connection
//! is available the hover includes title, severity/status and other metadata;
//! otherwise a plain link to the bug tracker is shown.

use rowan::ast::AstNode;
use text_size::{TextRange, TextSize};
use tower_lsp_server::ls_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};

use crate::bugs::{DebbugsBugSummary, LaunchpadBugSummary, SharedBugCache};

/// Bug reference found at a cursor position.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BugRef {
    Debian(u32),
    Launchpad(u32),
}

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
    let bug_ref = {
        let changelog = debian_changelog::ChangeLog::cast(parse.syntax_node())?;
        let offset = crate::position::try_position_to_offset(source_text, position)?;
        let entry = changelog.entry_at_offset(offset)?;
        find_bug_ref_in_entry(&entry, offset)?
    };

    match &bug_ref {
        BugRef::Debian(id) => {
            let summary = bug_cache.write().await.get_debian_bug_summary(*id).await;
            Some(match summary {
                Some(s) => make_debian_hover(&s),
                None => make_fallback_hover(&bug_ref),
            })
        }
        BugRef::Launchpad(id) => {
            let summary = bug_cache.write().await.get_launchpad_bug_summary(*id).await;
            Some(match summary {
                Some(s) => make_launchpad_hover(&s),
                None => make_fallback_hover(&bug_ref),
            })
        }
    }
}

// ------------------------------------------------------------------
// Bug-reference detection
// ------------------------------------------------------------------

/// Find a complete bug number at the given offset in a changelog entry.
fn find_bug_ref_in_entry(entry: &debian_changelog::Entry, offset: TextSize) -> Option<BugRef> {
    let detail = match entry.syntax().token_at_offset(offset) {
        rowan::TokenAtOffset::Single(token) => Some(token),
        rowan::TokenAtOffset::Between(left, right) => {
            if left.kind() == debian_changelog::SyntaxKind::DETAIL {
                Some(left)
            } else if right.kind() == debian_changelog::SyntaxKind::DETAIL {
                Some(right)
            } else {
                None
            }
        }
        rowan::TokenAtOffset::None => None,
    }?;

    if detail.kind() != debian_changelog::SyntaxKind::DETAIL {
        return None;
    }

    if let Some(id) =
        find_bug_number_at_offset(detail.text(), detail.text_range(), offset, "closes:")
    {
        return Some(BugRef::Debian(id));
    }

    if let Some(id) = find_bug_number_at_offset(detail.text(), detail.text_range(), offset, "lp:") {
        return Some(BugRef::Launchpad(id));
    }

    None
}

/// Find the complete bug number at the cursor offset within a detail line,
/// given a case-insensitive marker (e.g. `"closes:"` or `"lp:"`).
///
/// Supports comma-separated lists like `Closes: #123, #456`.
fn find_bug_number_at_offset(
    detail_text: &str,
    detail_range: TextRange,
    offset: TextSize,
    marker: &str,
) -> Option<u32> {
    if offset < detail_range.start() || offset > detail_range.end() {
        return None;
    }

    let relative_offset: usize = (offset - detail_range.start()).into();
    let mut rel = std::cmp::min(relative_offset, detail_text.len());
    while rel > 0 && !detail_text.is_char_boundary(rel) {
        rel -= 1;
    }

    let lower = detail_text.to_ascii_lowercase();

    // Find the last marker occurrence at a word boundary before the cursor.
    let mut marker_pos = None;
    for (idx, _) in lower.match_indices(marker) {
        if idx > rel {
            break;
        }
        let is_word_boundary = if idx == 0 {
            true
        } else {
            let prev = lower.as_bytes()[idx - 1];
            !(prev.is_ascii_alphanumeric() || prev == b'-' || prev == b'_')
        };
        if is_word_boundary {
            marker_pos = Some(idx);
        }
    }
    let marker_pos = marker_pos?;
    let after_marker = &detail_text[marker_pos + marker.len()..];

    // Walk comma-separated fragments and return the bug number whose
    // fragment contains the cursor.
    let mut pos = marker_pos + marker.len();
    for fragment in after_marker.split(',') {
        let fragment_start = pos;
        let fragment_end = pos + fragment.len();

        let trimmed = fragment.trim();
        if trimmed.is_empty() {
            pos = fragment_end + 1;
            continue;
        }

        // Strip optional '#' prefix and extract only the leading digits,
        // ignoring any trailing characters like ')'.
        let after_hash = trimmed.strip_prefix('#').unwrap_or(trimmed);
        let digits_str: String = after_hash
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if digits_str.is_empty() {
            break;
        }

        if rel >= fragment_start && rel <= fragment_end {
            return digits_str.parse().ok();
        }

        pos = fragment_end + 1;
    }

    None
}

// ------------------------------------------------------------------
// Hover rendering
// ------------------------------------------------------------------

/// Minimal hover shown when bug details are not available.
fn make_fallback_hover(bug_ref: &BugRef) -> Hover {
    let (label, url) = match bug_ref {
        BugRef::Debian(id) => (
            format!("Debian Bug #{}", id),
            format!("https://bugs.debian.org/{}", id),
        ),
        BugRef::Launchpad(id) => (
            format!("Launchpad Bug #{}", id),
            format!("https://bugs.launchpad.net/bugs/{}", id),
        ),
    };

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("**[{}]({})** ", label, url),
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

    fn parse_for(text: &str) -> debian_changelog::Parse<debian_changelog::ChangeLog> {
        debian_changelog::ChangeLog::parse(text)
    }

    /// Helper: detect the bug ref without going through the async path.
    fn find_bug_ref(text: &str, byte_offset: usize) -> Option<BugRef> {
        let parsed = parse_for(text);
        let changelog = debian_changelog::ChangeLog::cast(parsed.syntax_node())?;
        let offset = TextSize::try_from(byte_offset).ok()?;
        let entry = changelog.entry_at_offset(offset)?;
        find_bug_ref_in_entry(&entry, offset)
    }

    #[test]
    fn test_detect_closes_bug() {
        let text = "foo (1.0-1) unstable; urgency=medium\n\n  * Fixed a bug. (Closes: #123456)\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let offset = text.find("#123456").unwrap() + 1;
        assert_eq!(find_bug_ref(text, offset), Some(BugRef::Debian(123456)));
    }

    #[test]
    fn test_detect_lp_bug() {
        let text = "foo (1.0-1) unstable; urgency=medium\n\n  * Fixed a bug. (LP: #987654)\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let offset = text.find("#987654").unwrap() + 1;
        assert_eq!(find_bug_ref(text, offset), Some(BugRef::Launchpad(987654)));
    }

    #[test]
    fn test_detect_multiple_closes_first() {
        let text = "foo (1.0-1) unstable; urgency=medium\n\n  * Fixed bugs. (Closes: #111, #222)\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let offset = text.find("#111").unwrap() + 1;
        assert_eq!(find_bug_ref(text, offset), Some(BugRef::Debian(111)));
    }

    #[test]
    fn test_detect_multiple_closes_second() {
        let text = "foo (1.0-1) unstable; urgency=medium\n\n  * Fixed bugs. (Closes: #111, #222)\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let offset = text.find("#222").unwrap() + 1;
        assert_eq!(find_bug_ref(text, offset), Some(BugRef::Debian(222)));
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
        let hover = make_fallback_hover(&BugRef::Debian(42));
        assert_eq!(
            hover_markdown(&hover),
            "**[Debian Bug #42](https://bugs.debian.org/42)** "
        );
    }

    #[test]
    fn test_fallback_hover_launchpad() {
        let hover = make_fallback_hover(&BugRef::Launchpad(42));
        assert_eq!(
            hover_markdown(&hover),
            "**[Launchpad Bug #42](https://bugs.launchpad.net/bugs/42)** "
        );
    }
}
