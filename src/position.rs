use text_size::{TextRange, TextSize};
use tower_lsp_server::ls_types::{Position, Range};

/// Convert TextSize to LSP Position
pub fn offset_to_position(text: &str, offset: TextSize) -> Position {
    let mut line = 0;
    let mut line_start_offset = TextSize::from(0);

    for (i, ch) in text.char_indices() {
        let current_offset = TextSize::try_from(i).unwrap();

        if current_offset >= offset {
            break;
        }

        if ch == '\n' {
            line += 1;
            line_start_offset = TextSize::try_from(i + 1).unwrap();
        }
    }

    let character = (offset - line_start_offset).into();

    Position { line, character }
}

/// Convert TextRange to LSP Range
pub fn text_range_to_lsp_range(text: &str, range: TextRange) -> Range {
    Range {
        start: offset_to_position(text, range.start()),
        end: offset_to_position(text, range.end()),
    }
}

/// Convert LSP Position to TextSize
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

    // If character is beyond the end of the target line, return an error.
    let line_end = text[line_start..]
        .find('\n')
        .map(|rel| line_start + rel)
        .unwrap_or(text.len());
    let requested = line_start.checked_add(position.character as usize)?;
    if requested > line_end {
        return None;
    }

    TextSize::try_from(requested).ok()
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
}
