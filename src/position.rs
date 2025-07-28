use text_size::{TextRange, TextSize};
use tower_lsp::lsp_types::{Position, Range};

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
