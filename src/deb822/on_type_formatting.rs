use tower_lsp_server::ls_types::{Position, TextEdit};

/// Generate on-type formatting edits for deb822 files.
///
/// Handles two cases:
/// - After typing `:` at the end of a field name, insert a trailing space
/// - After typing a newline inside a multi-line field value, insert a leading space for
///   continuation
pub fn on_type_formatting(
    deb822: &deb822_lossless::Deb822,
    source_text: &str,
    position: Position,
    ch: &str,
) -> Option<Vec<TextEdit>> {
    match ch {
        ":" => on_type_colon(deb822, source_text, position),
        "\n" => on_type_newline(deb822, source_text, position),
        _ => None,
    }
}

/// After typing `:`, check if the CST shows we just completed a field separator and insert
/// a space.
fn on_type_colon(
    deb822: &deb822_lossless::Deb822,
    source_text: &str,
    position: Position,
) -> Option<Vec<TextEdit>> {
    // The position is after the colon was typed. Look up the entry at the colon position.
    let colon_col = position.character.checked_sub(1)?;
    let entry = deb822.entry_at_line_col(position.line as usize, colon_col as usize)?;

    // Verify this entry has a colon (i.e. the colon we typed is the field separator).
    let colon_range = entry.colon_range()?;

    // Convert the colon's byte offset to a line/column to confirm it matches the typed position.
    let colon_offset: usize = colon_range.start().into();
    let colon_line = source_text[..colon_offset].matches('\n').count();
    if colon_line != position.line as usize {
        return None;
    }

    // Check that there isn't already a space after the colon.
    let after_colon_offset: usize = colon_range.end().into();
    if source_text[after_colon_offset..].starts_with(' ') {
        return None;
    }

    Some(vec![TextEdit {
        range: tower_lsp_server::ls_types::Range {
            start: position,
            end: position,
        },
        new_text: " ".to_string(),
    }])
}

/// After typing a newline, if the previous line is part of a field entry,
/// insert a leading space for continuation.
fn on_type_newline(
    deb822: &deb822_lossless::Deb822,
    source_text: &str,
    position: Position,
) -> Option<Vec<TextEdit>> {
    if position.line == 0 {
        return None;
    }

    let prev_line_idx = (position.line - 1) as usize;
    let prev_line = source_text.lines().nth(prev_line_idx)?;

    // Use the CST to check if the previous line is inside a field entry.
    // Use column 0 for field lines and column 1 for continuation lines (which start
    // with whitespace, so column 0 is indent, not inside the entry key).
    let col = if prev_line.starts_with(' ') || prev_line.starts_with('\t') {
        1
    } else {
        0
    };
    let entry = deb822.entry_at_line_col(prev_line_idx, col)?;

    // If the entry has no colon yet, the user is still typing a field name — don't insert.
    entry.colon_range()?;

    // Check if the current line already starts with whitespace.
    let current_line = source_text
        .lines()
        .nth(position.line as usize)
        .unwrap_or("");
    if current_line.starts_with(' ') || current_line.starts_with('\t') {
        return None;
    }

    // Don't add indentation if the current line has non-whitespace content.
    if !current_line.is_empty() {
        return None;
    }

    Some(vec![TextEdit {
        range: tower_lsp_server::ls_types::Range {
            start: position,
            end: position,
        },
        new_text: " ".to_string(),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> deb822_lossless::Deb822 {
        deb822_lossless::Deb822::parse(text).tree()
    }

    #[test]
    fn test_colon_after_field_name_inserts_space() {
        // Position is after the colon (cursor position after typing)
        let text = "Source:\n";
        let deb822 = parse(text);
        let edits = on_type_formatting(&deb822, text, Position::new(0, 7), ":").unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, " ");
        assert_eq!(edits[0].range.start, Position::new(0, 7));
    }

    #[test]
    fn test_colon_after_hyphenated_field_name_inserts_space() {
        let text = "Vcs-Bzr:\n";
        let deb822 = parse(text);
        let edits = on_type_formatting(&deb822, text, Position::new(0, 8), ":").unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, " ");
        assert_eq!(edits[0].range.start, Position::new(0, 8));
    }

    #[test]
    fn test_colon_with_existing_space_no_edit() {
        let text = "Source: foo\n";
        let deb822 = parse(text);
        let result = on_type_formatting(&deb822, text, Position::new(0, 7), ":");
        assert!(result.is_none());
    }

    #[test]
    fn test_colon_in_value_no_edit() {
        let text = "Source: foo:bar\n";
        let deb822 = parse(text);
        // Typing a colon inside a value (after the field colon) should not trigger
        let result = on_type_formatting(&deb822, text, Position::new(0, 11), ":");
        assert!(result.is_none());
    }

    #[test]
    fn test_colon_on_continuation_line_no_edit() {
        let text = "Description: foo\n bar:\n";
        let deb822 = parse(text);
        let result = on_type_formatting(&deb822, text, Position::new(1, 5), ":");
        assert!(result.is_none());
    }

    #[test]
    fn test_newline_after_field_value_inserts_space() {
        let text = "Description: foo\n\n";
        let deb822 = parse(text);
        let edits = on_type_formatting(&deb822, text, Position::new(1, 0), "\n").unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, " ");
        assert_eq!(edits[0].range.start, Position::new(1, 0));
    }

    #[test]
    fn test_newline_after_continuation_line_inserts_space() {
        let text = "Description: foo\n bar\n\n";
        let deb822 = parse(text);
        let edits = on_type_formatting(&deb822, text, Position::new(2, 0), "\n").unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, " ");
    }

    #[test]
    fn test_newline_at_start_of_file_no_edit() {
        let text = "\n";
        let deb822 = parse(text);
        let result = on_type_formatting(&deb822, text, Position::new(0, 0), "\n");
        assert!(result.is_none());
    }

    #[test]
    fn test_newline_after_empty_line_no_edit() {
        let text = "Source: foo\n\n\n";
        let deb822 = parse(text);
        // Line 1 is empty (paragraph separator), line 2 is the new line
        let result = on_type_formatting(&deb822, text, Position::new(2, 0), "\n");
        assert!(result.is_none());
    }

    #[test]
    fn test_newline_already_indented_no_edit() {
        let text = "Description: foo\n bar\n";
        let deb822 = parse(text);
        // The current line already starts with a space
        let result = on_type_formatting(&deb822, text, Position::new(1, 0), "\n");
        assert!(result.is_none());
    }
}
