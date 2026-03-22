//! Selection range generation for deb822 files.
//!
//! Provides hierarchical selection expansion:
//! 1. Field value only
//! 2. Field name + value (entire entry)
//! 3. Entire paragraph
//! 4. Complete file

use deb822_lossless::Deb822;
use text_size::TextSize;
use tower_lsp_server::ls_types::{Position, Range, SelectionRange};

use crate::position::{offset_to_position, text_range_to_lsp_range, try_position_to_offset};

/// Generate selection ranges for the given positions in a deb822 document.
pub fn generate_selection_ranges(
    deb822: &Deb822,
    source_text: &str,
    positions: &[Position],
) -> Vec<SelectionRange> {
    let file_range = Range::new(
        Position::new(0, 0),
        offset_to_position(source_text, TextSize::try_from(source_text.len()).unwrap()),
    );

    positions
        .iter()
        .map(|pos| {
            let file_sel = SelectionRange {
                range: file_range,
                parent: None,
            };

            let Some(offset) = try_position_to_offset(source_text, *pos) else {
                return file_sel;
            };
            let offset = TextSize::from(u32::from(offset));

            let Some(para) = deb822.paragraph_at_position(offset) else {
                return file_sel;
            };

            let para_range = text_range_to_lsp_range(source_text, para.text_range());
            let para_sel = SelectionRange {
                range: para_range,
                parent: Some(Box::new(file_sel)),
            };

            let Some(entry) = para.entry_at_position(offset) else {
                return para_sel;
            };

            let entry_range = text_range_to_lsp_range(source_text, entry.text_range());
            let entry_sel = SelectionRange {
                range: entry_range,
                parent: Some(Box::new(para_sel)),
            };

            if let Some(value_range) = entry.value_range() {
                let value_lsp_range = text_range_to_lsp_range(source_text, value_range);
                if value_range.contains(offset) {
                    return SelectionRange {
                        range: value_lsp_range,
                        parent: Some(Box::new(entry_sel)),
                    };
                }
            }

            if let Some(key_range) = entry.key_range() {
                let key_lsp_range = text_range_to_lsp_range(source_text, key_range);
                if key_range.contains(offset) {
                    return SelectionRange {
                        range: key_lsp_range,
                        parent: Some(Box::new(entry_sel)),
                    };
                }
            }

            entry_sel
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selection_range_in_value() {
        let text = "Source: foo\nMaintainer: Test <test@example.com>\n";
        let parsed = Deb822::parse(text);
        let deb822 = parsed.tree();

        // Position in the value "foo" on line 0, col 8
        let ranges = generate_selection_ranges(&deb822, text, &[Position::new(0, 8)]);
        assert_eq!(ranges.len(), 1);

        let sel = &ranges[0];
        // Innermost: value range
        assert_eq!(sel.range.start.line, 0);
        assert_eq!(sel.range.start.character, 8);

        // Parent: entry range
        let entry_sel = sel.parent.as_ref().unwrap();
        assert_eq!(entry_sel.range.start.line, 0);
        assert_eq!(entry_sel.range.start.character, 0);

        // Grandparent: paragraph range
        let para_sel = entry_sel.parent.as_ref().unwrap();
        assert_eq!(para_sel.range.start.line, 0);

        // Great-grandparent: file range
        let file_sel = para_sel.parent.as_ref().unwrap();
        assert_eq!(file_sel.range.start.line, 0);
        assert_eq!(file_sel.range.start.character, 0);
        assert!(file_sel.parent.is_none());
    }

    #[test]
    fn test_selection_range_in_key() {
        let text = "Source: foo\n";
        let parsed = Deb822::parse(text);
        let deb822 = parsed.tree();

        // Position in "Source" at col 2
        let ranges = generate_selection_ranges(&deb822, text, &[Position::new(0, 2)]);
        assert_eq!(ranges.len(), 1);

        let sel = &ranges[0];
        // Innermost: key range ("Source")
        assert_eq!(sel.range.start.character, 0);
        assert_eq!(sel.range.end.character, 6);

        // Parent: entry
        let entry_sel = sel.parent.as_ref().unwrap();
        assert_eq!(entry_sel.range.start.line, 0);

        // Grandparent: paragraph
        assert!(entry_sel.parent.is_some());
    }

    #[test]
    fn test_selection_range_multiple_paragraphs() {
        let text = "\
Source: foo
Maintainer: Test <test@example.com>

Package: foo
Architecture: any
";
        let parsed = Deb822::parse(text);
        let deb822 = parsed.tree();

        // Position in second paragraph, "Architecture" value "any"
        let ranges = generate_selection_ranges(&deb822, text, &[Position::new(4, 15)]);
        assert_eq!(ranges.len(), 1);

        let sel = &ranges[0];
        // Value range
        assert_eq!(sel.range.start.line, 4);

        // Entry
        let entry_sel = sel.parent.as_ref().unwrap();
        assert_eq!(entry_sel.range.start.line, 4);

        // Paragraph starts at line 3
        let para_sel = entry_sel.parent.as_ref().unwrap();
        assert_eq!(para_sel.range.start.line, 3);

        // File
        let file_sel = para_sel.parent.as_ref().unwrap();
        assert_eq!(file_sel.range.start.line, 0);
    }

    #[test]
    fn test_selection_range_multiple_positions() {
        let text = "Source: foo\nMaintainer: Test <test@example.com>\n";
        let parsed = Deb822::parse(text);
        let deb822 = parsed.tree();

        let ranges =
            generate_selection_ranges(&deb822, text, &[Position::new(0, 8), Position::new(1, 13)]);
        assert_eq!(ranges.len(), 2);
    }

    #[test]
    fn test_selection_range_empty_file() {
        let text = "";
        let parsed = Deb822::parse(text);
        let deb822 = parsed.tree();

        let ranges = generate_selection_ranges(&deb822, text, &[Position::new(0, 0)]);
        assert_eq!(ranges.len(), 1);
        // Should return file range
        assert!(ranges[0].parent.is_none());
    }
}
