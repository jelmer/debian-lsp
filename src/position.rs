use tower_lsp::lsp_types::{Position, Range};

/// Convert byte offset to LSP position
pub fn offset_to_position(text: &str, offset: usize) -> Position {
    let mut line = 0;
    let mut character = 0;

    for (i, ch) in text.char_indices() {
        if i >= offset {
            break;
        }

        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += 1;
        }
    }

    Position { line, character }
}

/// Convert byte range to LSP range
pub fn range_to_lsp_range(text: &str, range: std::ops::Range<usize>) -> Range {
    Range {
        start: offset_to_position(text, range.start),
        end: offset_to_position(text, range.end),
    }
}
