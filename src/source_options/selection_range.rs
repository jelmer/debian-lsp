//! Selection range generation for debian/source/options files.
//!
//! Hierarchy: current line → file.

use crate::position::Source;
use text_size::TextSize;
use tower_lsp_server::ls_types::{Position, Range, SelectionRange};

/// Generate selection ranges for a debian/source/options file.
pub fn generate_selection_ranges(src: Source<'_>, positions: &[Position]) -> Vec<SelectionRange> {
    let file_range = Range::new(
        Position::new(0, 0),
        src.offset_to_position(TextSize::from(src.text.len() as u32)),
    );

    positions
        .iter()
        .map(|pos| {
            let file_sel = SelectionRange {
                range: file_range,
                parent: None,
            };

            let line = pos.line as usize;
            let line_text = match src.text.lines().nth(line) {
                Some(t) => t,
                None => return file_sel,
            };

            if line_text.trim().is_empty() {
                return file_sel;
            }

            // Calculate byte offset of the line start
            let line_start: usize = src
                .text
                .lines()
                .take(line)
                .map(|l| l.len() + 1) // +1 for newline
                .sum();

            let line_range = Range::new(
                src.offset_to_position(TextSize::from(line_start as u32)),
                src.offset_to_position(TextSize::from((line_start + line_text.len()) as u32)),
            );

            SelectionRange {
                range: line_range,
                parent: Some(Box::new(file_sel)),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selection_on_option_line() {
        let text = "compression = xz\nsingle-debian-patch\n";
        let idx = crate::position::LineIndex::new(text);
        let ranges = generate_selection_ranges(Source::new(text, &idx), &[Position::new(0, 5)]);
        assert_eq!(ranges.len(), 1);

        let sel = &ranges[0];
        // Line range
        assert_eq!(sel.range.start.line, 0);
        assert_eq!(sel.range.end.line, 0);

        // Parent: file
        let file_sel = sel.parent.as_ref().unwrap();
        assert_eq!(file_sel.range.start.line, 0);
        assert!(file_sel.parent.is_none());
    }

    #[test]
    fn test_selection_on_second_line() {
        let text = "compression = xz\nsingle-debian-patch\n";
        let idx = crate::position::LineIndex::new(text);
        let ranges = generate_selection_ranges(Source::new(text, &idx), &[Position::new(1, 3)]);
        assert_eq!(ranges.len(), 1);

        let sel = &ranges[0];
        assert_eq!(sel.range.start.line, 1);
        assert_eq!(sel.range.end.line, 1);
        assert!(sel.parent.is_some());
    }

    #[test]
    fn test_selection_on_empty_line() {
        let text = "compression = xz\n\nsingle-debian-patch\n";
        let idx = crate::position::LineIndex::new(text);
        let ranges = generate_selection_ranges(Source::new(text, &idx), &[Position::new(1, 0)]);
        assert_eq!(ranges.len(), 1);

        // Empty line falls back to file range
        assert!(ranges[0].parent.is_none());
    }

    #[test]
    fn test_selection_on_comment() {
        let text = "# a comment\ncompression = xz\n";
        let idx = crate::position::LineIndex::new(text);
        let ranges = generate_selection_ranges(Source::new(text, &idx), &[Position::new(0, 3)]);
        assert_eq!(ranges.len(), 1);

        let sel = &ranges[0];
        assert_eq!(sel.range.start.line, 0);
        assert!(sel.parent.is_some());
    }

    #[test]
    fn test_empty_file() {
        let text = "";
        let idx = crate::position::LineIndex::new(text);
        let ranges = generate_selection_ranges(Source::new(text, &idx), &[Position::new(0, 0)]);
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].parent.is_none());
    }
}
