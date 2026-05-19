use ::debian_workspace::action::ChangelogAction;
use debian_changelog::ChangeLog;
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{Range, TextEdit};

pub fn changelog_action_to_text_edits(
    action: &ChangelogAction,
    changelog: &ChangeLog,
    original_src: crate::position::Source<'_>,
) -> Vec<TextEdit> {
    match action {
        ChangelogAction::SetEntryDate {
            version, rfc2822, ..
        } => {
            let Some(entry) = find_changelog_entry(changelog, version) else {
                return Vec::new();
            };
            let Some(timestamp) = entry.timestamp_node() else {
                return Vec::new();
            };
            if entry.timestamp().as_deref() == Some(rfc2822.as_str()) {
                return Vec::new();
            }
            let lsp_range = original_src.text_range_to_lsp_range(timestamp.syntax().text_range());
            vec![TextEdit {
                range: lsp_range,
                new_text: rfc2822.clone(),
            }]
        }
        ChangelogAction::ReplaceEntryChanges { version, lines, .. } => {
            let Some(entry) = find_changelog_entry(changelog, version) else {
                return Vec::new();
            };
            let current: Vec<String> = entry.change_lines().collect();
            if current == *lines {
                return Vec::new();
            }
            let Some(range) = entry_change_block_range(&entry) else {
                return Vec::new();
            };
            let lsp_range = original_src.text_range_to_lsp_range(range);
            vec![TextEdit {
                range: lsp_range,
                new_text: render_changelog_change_block(lines),
            }]
        }
        ChangelogAction::RemoveBullet {
            version,
            author,
            text,
            occurrence,
            ..
        } => {
            let Some(range) =
                find_bullet_range(changelog, original_src, version, author, text, *occurrence)
            else {
                return Vec::new();
            };
            let lsp_range = original_src.text_range_to_lsp_range(range);
            vec![TextEdit {
                range: lsp_range,
                new_text: String::new(),
            }]
        }
        ChangelogAction::ReplaceBullet {
            version,
            author,
            text,
            occurrence,
            new_lines,
            ..
        } => {
            if text == &new_lines.join("\n") {
                return Vec::new();
            }
            let Some(range) =
                find_bullet_range(changelog, original_src, version, author, text, *occurrence)
            else {
                return Vec::new();
            };
            let lsp_range = original_src.text_range_to_lsp_range(range);
            vec![TextEdit {
                range: lsp_range,
                new_text: render_bullet_block(new_lines),
            }]
        }
        ChangelogAction::SetEntryVersion {
            version,
            new_version,
            ..
        } => {
            if version == new_version {
                return Vec::new();
            }
            let Some(entry) = find_changelog_entry(changelog, version) else {
                return Vec::new();
            };
            let Some(range) = entry_version_token_range(&entry) else {
                return Vec::new();
            };
            let lsp_range = original_src.text_range_to_lsp_range(range);
            vec![TextEdit {
                range: lsp_range,
                new_text: format!("({})", new_version),
            }]
        }
    }
}

pub(super) fn changelog_action_range(
    action: &ChangelogAction,
    changelog: &ChangeLog,
    anchor_src: crate::position::Source<'_>,
) -> Option<Range> {
    match action {
        ChangelogAction::SetEntryDate { version, .. } => {
            let entry = find_changelog_entry(changelog, version)?;
            let ts = entry.timestamp_node()?;
            Some(anchor_src.text_range_to_lsp_range(ts.syntax().text_range()))
        }
        ChangelogAction::ReplaceEntryChanges { version, .. } => {
            let entry = find_changelog_entry(changelog, version)?;
            let range = entry_change_block_range(&entry)?;
            Some(anchor_src.text_range_to_lsp_range(range))
        }
        ChangelogAction::RemoveBullet {
            version,
            author,
            text,
            occurrence,
            ..
        }
        | ChangelogAction::ReplaceBullet {
            version,
            author,
            text,
            occurrence,
            ..
        } => {
            let range =
                find_bullet_range(changelog, anchor_src, version, author, text, *occurrence)?;
            Some(anchor_src.text_range_to_lsp_range(range))
        }
        ChangelogAction::SetEntryVersion { version, .. } => {
            let entry = find_changelog_entry(changelog, version)?;
            let range = entry_version_token_range(&entry)?;
            Some(anchor_src.text_range_to_lsp_range(range))
        }
    }
}

pub(super) fn find_changelog_entry(
    changelog: &ChangeLog,
    version: &str,
) -> Option<debian_changelog::Entry> {
    changelog.iter().find(|e| {
        e.version()
            .map(|v| v.to_string() == version)
            .unwrap_or(false)
    })
}

/// Locate the byte range of an entry's `(version)` token in the changelog
/// header (e.g. `(2.6.0-1)`). Returns `None` if the entry has no version
/// token (a malformed header).
fn entry_version_token_range(entry: &debian_changelog::Entry) -> Option<rowan::TextRange> {
    use debian_changelog::SyntaxKind;
    let header = entry.header()?;
    header
        .syntax()
        .children_with_tokens()
        .find(|tok| tok.kind() == SyntaxKind::VERSION)
        .map(|tok| tok.text_range())
}

/// Compute the rowan byte range covering all `EntryBody` children of an
/// entry — i.e. the change-lines block. Spans from the first `EntryBody`
/// to the last, picking up any non-body siblings (empty-line separators)
/// that sit between them, and excluding the surrounding header/footer.
fn entry_change_block_range(entry: &debian_changelog::Entry) -> Option<rowan::TextRange> {
    use debian_changelog::EntryBody;
    let bodies: Vec<_> = entry
        .syntax()
        .children()
        .filter_map(EntryBody::cast)
        .collect();
    let first = bodies.first()?;
    let last = bodies.last()?;
    Some(rowan::TextRange::new(
        first.syntax().text_range().start(),
        last.syntax().text_range().end(),
    ))
}

/// Render a list of change-line strings as the textual block they replace.
/// Each line is emitted verbatim, with a trailing newline. An empty `lines`
/// slice produces an empty string.
fn render_changelog_change_block(lines: &[String]) -> String {
    let mut out = String::new();
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Find the bullet matching `(version, author, text, occurrence)` and
/// return its byte range in the file. Walks the same iteration order
/// `apply_changelog_group` uses, then derives the range from the
/// `Change`'s reported line numbers (relative to the parent entry).
fn find_bullet_range(
    changelog: &ChangeLog,
    original_src: crate::position::Source<'_>,
    version: &str,
    author: &Option<String>,
    text: &str,
    occurrence: usize,
) -> Option<rowan::TextRange> {
    use debian_changelog::iter_changes_by_author;
    let mut seen = 0usize;
    for change in iter_changes_by_author(changelog) {
        if change.version().map(|v| v.to_string()).as_deref() != Some(version) {
            continue;
        }
        for bullet in change.split_into_bullets() {
            let bullet_author = bullet.author().map(|s| s.to_string());
            let bullet_text = bullet.lines().join("\n");
            if bullet_author == *author && bullet_text == *text {
                if seen == occurrence {
                    return bullet_byte_range(&bullet, original_src);
                }
                seen += 1;
            }
        }
    }
    None
}

/// Compute the byte range covering a bullet's lines in the source file.
/// Uses the bullet's reported start line (file-relative, 0-indexed) plus
/// the count of lines it occupies, walking `original_src` to map back to
/// byte offsets. Each bullet line is removed in full — leading indent
/// through trailing newline.
fn bullet_byte_range(
    bullet: &debian_changelog::Change,
    original_src: crate::position::Source<'_>,
) -> Option<rowan::TextRange> {
    let original_text = original_src.text;
    let start_line = bullet.line()?;
    let line_count = bullet.lines().len();
    let abs_start = nth_line_start(original_text, start_line)?;
    let abs_end =
        nth_line_start(original_text, start_line + line_count).unwrap_or(original_text.len());
    Some(rowan::TextRange::new(
        (abs_start as u32).into(),
        (abs_end as u32).into(),
    ))
}

/// Return the byte offset of the start of the n-th 0-indexed line in
/// `text`. `nth_line_start(text, 0)` is `Some(0)`. Returns `None` if the
/// text has fewer than `n` newlines.
fn nth_line_start(text: &str, n: usize) -> Option<usize> {
    if n == 0 {
        return Some(0);
    }
    let mut count = 0;
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            count += 1;
            if count == n {
                return Some(i + 1);
            }
        }
    }
    None
}

fn render_bullet_block(new_lines: &[String]) -> String {
    let mut out = String::new();
    for line in new_lines {
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }
    out
}
