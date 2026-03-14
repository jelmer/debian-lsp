use text_size::{TextRange, TextSize};
use tower_lsp_server::ls_types::{Position, Range};

/// Return the UTF-16 code unit length of a string.
pub fn utf16_len(s: &str) -> u32 {
    s.chars().map(|c| c.len_utf16() as u32).sum()
}

/// Convert TextSize (byte offset) to LSP Position (line, UTF-16 code unit offset)
pub fn offset_to_position(text: &str, offset: TextSize) -> Position {
    let mut line = 0u32;
    let mut utf16_col = 0u32;

    for (i, ch) in text.char_indices() {
        let current_offset = TextSize::try_from(i).unwrap();

        if current_offset >= offset {
            break;
        }

        if ch == '\n' {
            line += 1;
            utf16_col = 0;
        } else {
            utf16_col += ch.len_utf16() as u32;
        }
    }

    Position {
        line,
        character: utf16_col,
    }
}

/// Convert TextRange to LSP Range
pub fn text_range_to_lsp_range(text: &str, range: TextRange) -> Range {
    Range {
        start: offset_to_position(text, range.start()),
        end: offset_to_position(text, range.end()),
    }
}

/// Convert LSP Position (line, UTF-16 code unit offset) to TextSize (byte offset)
pub fn try_position_to_offset(text: &str, position: Position) -> Option<TextSize> {
    let mut line = 0u32;
    let mut line_start = 0usize;

    // Find the byte offset where the requested line starts.
    for (i, ch) in text.char_indices() {
        if line == position.line {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = i + 1;
        }
    }

    // If the requested line is beyond the available lines, return an error.
    if line < position.line {
        return None;
    }

    // Walk UTF-16 code units to find the byte offset.
    let mut utf16_col = 0u32;
    for (i, ch) in text[line_start..].char_indices() {
        if utf16_col >= position.character {
            return TextSize::try_from(line_start + i).ok();
        }
        if ch == '\n' {
            break;
        }
        utf16_col += ch.len_utf16() as u32;
    }

    // Character position is at or past end of line content.
    if utf16_col >= position.character {
        // Position is at the newline or end of text — find the byte offset.
        let line_end = text[line_start..]
            .find('\n')
            .map(|rel| line_start + rel)
            .unwrap_or(text.len());
        return TextSize::try_from(line_end).ok();
    }

    None
}

/// Convert LSP Range to TextRange
pub fn try_lsp_range_to_text_range(text: &str, range: &Range) -> Option<TextRange> {
    let start = try_position_to_offset(text, range.start)?;
    let end = try_position_to_offset(text, range.end)?;
    Some(TextRange::new(start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_try_position_to_offset_multiline_value_end() {
        let text = "Source: test\nSection: py\n";
        let offset = try_position_to_offset(text, Position::new(1, 11)).unwrap();
        let expected = TextSize::try_from(24usize).unwrap();
        assert_eq!(offset, expected);
    }

    #[test]
    fn test_try_position_to_offset_returns_none_for_out_of_range_character() {
        let text = "Source: test\nSection: py\n";
        let offset = try_position_to_offset(text, Position::new(1, 99));
        assert!(offset.is_none());
    }

    #[test]
    fn test_try_position_to_offset_returns_none_for_out_of_range_line() {
        let text = "Source: test\n";
        let offset = try_position_to_offset(text, Position::new(5, 0));
        assert!(offset.is_none());
    }

    #[test]
    fn test_try_lsp_range_to_text_range_valid() {
        let text = "Source: test\nSection: py\n";
        let range = Range::new(Position::new(1, 0), Position::new(1, 7));
        let text_range = try_lsp_range_to_text_range(text, &range).unwrap();

        let expected_start = TextSize::try_from(13usize).unwrap();
        let expected_end = TextSize::try_from(20usize).unwrap();
        assert_eq!(text_range.start(), expected_start);
        assert_eq!(text_range.end(), expected_end);
    }

    #[test]
    fn test_try_lsp_range_to_text_range_invalid_returns_none() {
        let text = "Source: test\n";
        let range = Range::new(Position::new(10, 0), Position::new(10, 1));
        assert!(try_lsp_range_to_text_range(text, &range).is_none());
    }

    #[test]
    fn test_offset_to_position_with_multibyte_chars() {
        // 'ĳ' is U+0133: 2 bytes in UTF-8, 1 code unit in UTF-16
        let text = "Vernooĳ rest";
        // 'V' at byte 0 -> col 0
        assert_eq!(
            offset_to_position(text, TextSize::from(0u32)),
            Position::new(0, 0)
        );
        // 'ĳ' starts at byte 6, col 6
        assert_eq!(
            offset_to_position(text, TextSize::from(6u32)),
            Position::new(0, 6)
        );
        // ' ' after 'ĳ' is at byte 8, but UTF-16 col 7
        assert_eq!(
            offset_to_position(text, TextSize::from(8u32)),
            Position::new(0, 7)
        );
        // 'r' of "rest" at byte 9, UTF-16 col 8
        assert_eq!(
            offset_to_position(text, TextSize::from(9u32)),
            Position::new(0, 8)
        );
    }

    #[test]
    fn test_try_position_to_offset_with_multibyte_chars() {
        let text = "Vernooĳ rest";
        // col 7 in UTF-16 -> byte 8 (the space after 'ĳ')
        let offset = try_position_to_offset(text, Position::new(0, 7)).unwrap();
        assert_eq!(offset, TextSize::from(8u32));
        // col 8 in UTF-16 -> byte 9 ('r')
        let offset = try_position_to_offset(text, Position::new(0, 8)).unwrap();
        assert_eq!(offset, TextSize::from(9u32));
    }

    #[test]
    fn test_utf16_len() {
        assert_eq!(utf16_len("hello"), 5);
        assert_eq!(utf16_len("Vernooĳ"), 7); // ĳ is 1 UTF-16 code unit
        assert_eq!(utf16_len(""), 0);
        // Emoji 😀 (U+1F600) is 2 UTF-16 code units (surrogate pair)
        assert_eq!(utf16_len("😀"), 2);
    }
}
