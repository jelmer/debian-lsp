use debian_changelog::{ChangeLog, SyntaxKind};
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{Position, TextEdit};

/// Generate on-type formatting edits for debian/changelog files.
///
/// Handles:
/// - After typing a newline inside a changelog entry body, insert appropriate indentation
///   (`  * ` for a new bullet or `    ` for continuation of the previous bullet)
/// - After typing `-` completing ` --` on a line inside an entry, insert a trailing space
///   to start the signature line (` -- `)
pub fn on_type_formatting(
    parse: &debian_changelog::Parse<ChangeLog>,
    source_text: &str,
    position: Position,
    ch: &str,
) -> Option<Vec<TextEdit>> {
    match ch {
        "\n" => on_type_newline(parse, source_text, position),
        "-" => on_type_dash(parse, source_text, position),
        _ => None,
    }
}

/// After typing `-`, check if the current line is ` --` following an entry without a footer.
/// If so, insert a trailing space to start the signature line.
fn on_type_dash(
    parse: &debian_changelog::Parse<ChangeLog>,
    source_text: &str,
    position: Position,
) -> Option<Vec<TextEdit>> {
    let lines: Vec<&str> = source_text.lines().collect();
    let line = lines.get(position.line as usize)?;

    // Check that the line so far is exactly " --"
    if line.trim_end() != " --" {
        return None;
    }

    // Find the nearest entry that ends at or before this line.
    let line_start: usize = source_text
        .lines()
        .take(position.line as usize)
        .map(|l| l.len() + 1)
        .sum();
    let line_offset = text_size::TextSize::from(line_start as u32);

    let changelog = parse.tree();

    // Look for an entry that contains this offset, or the last entry that ends
    // at or just before this offset.
    let entry = changelog
        .iter()
        .filter(|e| e.syntax().text_range().start() <= line_offset)
        .last()?;

    // Only offer signature completion if the entry doesn't already have a footer.
    if entry.footer().is_some() {
        return None;
    }

    // Insert a space after the "--"
    Some(vec![TextEdit {
        range: tower_lsp_server::ls_types::Range {
            start: position,
            end: position,
        },
        new_text: " ".to_string(),
    }])
}

/// After typing a newline, check if the cursor is inside an entry body and insert
/// appropriate indentation.
fn on_type_newline(
    parse: &debian_changelog::Parse<ChangeLog>,
    source_text: &str,
    position: Position,
) -> Option<Vec<TextEdit>> {
    if position.line == 0 {
        return None;
    }

    let changelog = parse.tree();

    // Compute the byte range of the previous line.
    let prev_line_idx = (position.line - 1) as usize;
    let prev_line_start: usize = source_text
        .lines()
        .take(prev_line_idx)
        .map(|l| l.len() + 1) // +1 for newline
        .sum();
    let prev_line = source_text.lines().nth(prev_line_idx)?;
    let prev_line_end = prev_line_start + prev_line.len();
    let prev_line_range = text_size::TextRange::new(
        text_size::TextSize::from(prev_line_start as u32),
        text_size::TextSize::from(prev_line_end as u32),
    );

    // Find the entry whose range contains the previous line.
    let entry = changelog.iter().find(|e| {
        let range = e.syntax().text_range();
        range.start() <= prev_line_range.start() && prev_line_range.start() < range.end()
    })?;

    // Find the last DETAIL token across all ENTRY_BODY nodes that overlaps
    // with the previous line.
    let mut last_detail_text = None;
    for element in entry.syntax().children() {
        if element.kind() != SyntaxKind::ENTRY_BODY {
            continue;
        }
        for child in element.descendants_with_tokens() {
            if let rowan::NodeOrToken::Token(token) = child {
                if token.kind() == SyntaxKind::DETAIL {
                    let token_range = token.text_range();
                    if token_range.start() < prev_line_range.end()
                        && token_range.end() > prev_line_range.start()
                    {
                        last_detail_text = Some(token.text().to_string());
                    }
                }
            }
        }
    }
    let detail_text = last_detail_text?;

    // Determine what to insert: if the detail starts with "* " or "- ", it's a bullet;
    // otherwise it's a continuation line.
    let new_text = if detail_text.starts_with("* ") || detail_text.starts_with("- ") {
        "  * "
    } else {
        "    "
    };

    // Don't insert if the current line already has non-whitespace content.
    let current_line = source_text
        .lines()
        .nth(position.line as usize)
        .unwrap_or("");
    if !current_line.trim().is_empty() {
        return None;
    }

    // Replace any existing whitespace on the current line (e.g. editor auto-indent).
    let line_start = Position {
        line: position.line,
        character: 0,
    };
    let line_end = Position {
        line: position.line,
        character: current_line.len() as u32,
    };

    Some(vec![TextEdit {
        range: tower_lsp_server::ls_types::Range {
            start: line_start,
            end: line_end,
        },
        new_text: new_text.to_string(),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> debian_changelog::Parse<ChangeLog> {
        ChangeLog::parse(text)
    }

    #[test]
    fn test_newline_after_bullet_inserts_new_bullet() {
        let text = "pkg (1.0-1) unstable; urgency=medium\n\n  * First change.\n\n";
        let parsed = parse(text);
        let edits = on_type_formatting(&parsed, text, Position::new(3, 0), "\n").unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "  * ");
    }

    #[test]
    fn test_newline_after_bullet_with_auto_indent() {
        // Simulates VSCode adding 2 spaces of auto-indent on the new line.
        let text = "pkg (1.0-1) unstable; urgency=medium\n\n  * First change.\n  \n";
        let parsed = parse(text);
        let edits = on_type_formatting(&parsed, text, Position::new(3, 2), "\n").unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "  * ");
        // Should replace the auto-indent
        assert_eq!(edits[0].range.start.character, 0);
        assert_eq!(edits[0].range.end.character, 2);
    }

    #[test]
    fn test_newline_after_continuation_inserts_continuation() {
        let text =
            "pkg (1.0-1) unstable; urgency=medium\n\n  * A long change that\n    continues here.\n\n";
        let parsed = parse(text);
        let edits = on_type_formatting(&parsed, text, Position::new(4, 0), "\n").unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "    ");
    }

    #[test]
    fn test_newline_after_header_no_edit() {
        let text = "pkg (1.0-1) unstable; urgency=medium\n\n";
        let parsed = parse(text);
        let result = on_type_formatting(&parsed, text, Position::new(1, 0), "\n");
        assert!(result.is_none());
    }

    #[test]
    fn test_newline_after_signature_no_edit() {
        let text =
            "pkg (1.0-1) unstable; urgency=medium\n\n  * Change.\n\n -- Foo <f@b>  Mon, 01 Jan 2024 00:00:00 +0000\n\n";
        let parsed = parse(text);
        let result = on_type_formatting(&parsed, text, Position::new(5, 0), "\n");
        assert!(result.is_none());
    }

    #[test]
    fn test_newline_at_start_of_file_no_edit() {
        let text = "\n";
        let parsed = parse(text);
        let result = on_type_formatting(&parsed, text, Position::new(0, 0), "\n");
        assert!(result.is_none());
    }

    #[test]
    fn test_newline_current_line_has_content_no_edit() {
        let text = "pkg (1.0-1) unstable; urgency=medium\n\n  * Change.\nfoo\n";
        let parsed = parse(text);
        let result = on_type_formatting(&parsed, text, Position::new(3, 0), "\n");
        assert!(result.is_none());
    }

    #[test]
    fn test_colon_ignored() {
        let text = "pkg (1.0-1) unstable; urgency=medium\n\n  * Change.\n\n -- Foo <f@b>  Mon, 01 Jan 2024 00:00:00 +0000\n";
        let parsed = parse(text);
        let result = on_type_formatting(&parsed, text, Position::new(0, 10), ":");
        assert!(result.is_none());
    }

    #[test]
    fn test_dash_completing_signature_prefix() {
        let text = "pkg (1.0-1) unstable; urgency=medium\n\n  * Change.\n\n --\n";
        let parsed = parse(text);
        let edits = on_type_formatting(&parsed, text, Position::new(4, 3), "-").unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, " ");
        assert_eq!(edits[0].range.start, Position::new(4, 3));
    }

    #[test]
    fn test_dash_in_entry_with_footer_no_edit() {
        let text = "pkg (1.0-1) unstable; urgency=medium\n\n  * Change.\n\n -- Foo <f@b>  Mon, 01 Jan 2024 00:00:00 +0000\n";
        let parsed = parse(text);
        // Typing "--" on line 4 where the footer already exists
        let result = on_type_formatting(&parsed, text, Position::new(4, 3), "-");
        assert!(result.is_none());
    }

    #[test]
    fn test_dash_not_signature_prefix() {
        // Just a dash somewhere in a bullet line
        let text = "pkg (1.0-1) unstable; urgency=medium\n\n  * foo-\n";
        let parsed = parse(text);
        let result = on_type_formatting(&parsed, text, Position::new(2, 8), "-");
        assert!(result.is_none());
    }
}
