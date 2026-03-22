//! Selection range generation for debian/source/options files.
//!
//! Hierarchy: current line → file.

use text_size::TextSize;
use tower_lsp_server::ls_types::{Position, Range, SelectionRange};

use crate::position::offset_to_position;

/// Generate selection ranges for a debian/source/options file.
pub fn generate_selection_ranges(source_text: &str, positions: &[Position]) -> Vec<SelectionRange> {
    let file_range = Range::new(
        Position::new(0, 0),
        offset_to_position(source_text, TextSize::from(source_text.len() as u32)),
    );

    positions
        .iter()
        .map(|pos| {
            let file_sel = SelectionRange {
                range: file_range,
                parent: None,
            };

            let line = pos.line as usize;
            let line_text = match source_text.lines().nth(line) {
                Some(t) => t,
                None => return file_sel,
            };

            if line_text.trim().is_empty() {
                return file_sel;
            }

            // Calculate byte offset of the line start
            let line_start: usize = source_text
                .lines()
                .take(line)
                .map(|l| l.len() + 1) // +1 for newline
                .sum();

            let line_range = Range::new(
                offset_to_position(source_text, TextSize::from(line_start as u32)),
                offset_to_position(
                    source_text,
                    TextSize::from((line_start + line_text.len()) as u32),
                ),
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
        let ranges = generate_selection_ranges(text, &[Position::new(0, 5)]);
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
        let ranges = generate_selection_ranges(text, &[Position::new(1, 3)]);
        assert_eq!(ranges.len(), 1);

        let sel = &ranges[0];
        assert_eq!(sel.range.start.line, 1);
        assert_eq!(sel.range.end.line, 1);
        assert!(sel.parent.is_some());
    }

    #[test]
    fn test_selection_on_empty_line() {
        let text = "compression = xz\n\nsingle-debian-patch\n";
        let ranges = generate_selection_ranges(text, &[Position::new(1, 0)]);
        assert_eq!(ranges.len(), 1);

        // Empty line falls back to file range
        assert!(ranges[0].parent.is_none());
    }

    #[test]
    fn test_selection_on_comment() {
        let text = "# a comment\ncompression = xz\n";
        let ranges = generate_selection_ranges(text, &[Position::new(0, 3)]);
        assert_eq!(ranges.len(), 1);

        let sel = &ranges[0];
        assert_eq!(sel.range.start.line, 0);
        assert!(sel.parent.is_some());
    }

    #[test]
    fn test_empty_file() {
        let text = "";
        let ranges = generate_selection_ranges(text, &[Position::new(0, 0)]);
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].parent.is_none());
    }
}
