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
pub fn position_to_offset(text: &str, position: Position) -> TextSize {
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

    // If the requested line is beyond the available lines, clamp to EOF.
    if line < position.line {
        return TextSize::try_from(text.len()).unwrap();
    }

    // Clamp character offset to the end of the target line (before '\n').
    let line_end = text[line_start..]
        .find('\n')
        .map(|rel| line_start + rel)
        .unwrap_or(text.len());
    let requested = line_start.saturating_add(position.character as usize);
    let clamped = requested.min(line_end);

    TextSize::try_from(clamped).unwrap()
}

/// Convert LSP Range to TextRange
pub fn lsp_range_to_text_range(text: &str, range: &Range) -> TextRange {
    let start = position_to_offset(text, range.start);
    let end = position_to_offset(text, range.end);
    TextRange::new(start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_to_offset_multiline_value_end() {
        let text = "Source: test\nSection: py\n";
        let offset = position_to_offset(text, Position::new(1, 11));
        let expected = TextSize::try_from(24usize).unwrap();
        assert_eq!(offset, expected);
    }

    #[test]
    fn test_position_to_offset_clamps_to_line_end() {
        let text = "Source: test\nSection: py\n";
        let offset = position_to_offset(text, Position::new(1, 99));
        let expected = TextSize::try_from(24usize).unwrap();
        assert_eq!(offset, expected);
    }

    #[test]
    fn test_position_to_offset_beyond_last_line_returns_eof() {
        let text = "Source: test\n";
        let offset = position_to_offset(text, Position::new(5, 0));
        let expected = TextSize::try_from(text.len()).unwrap();
        assert_eq!(offset, expected);
    }
}
