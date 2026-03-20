//! Selection range generation for Debian changelog files.
//!
//! Provides hierarchical selection expansion:
//! 1. Entry header, body, or footer
//! 2. Entire changelog entry
//! 3. Complete file

use debian_changelog::{ChangeLog, Parse};
use rowan::ast::AstNode;
use text_size::TextSize;
use tower_lsp_server::ls_types::{Position, Range, SelectionRange};

use crate::position::{offset_to_position, text_range_to_lsp_range, try_position_to_offset};

/// Generate selection ranges for the given positions in a changelog file.
pub fn generate_selection_ranges(
    parse: &Parse<ChangeLog>,
    source_text: &str,
    positions: &[Position],
) -> Vec<SelectionRange> {
    let changelog = parse.tree();
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

            let Some(offset) = try_position_to_offset(source_text, *pos) else {
                return file_sel;
            };

            // Find which entry contains this position.
            let Some(entry) = changelog.iter().find(|e| {
                let r = e.syntax().text_range();
                r.contains(offset) || r.end() == offset
            }) else {
                return file_sel;
            };

            let entry_range = text_range_to_lsp_range(source_text, entry.syntax().text_range());
            let entry_sel = SelectionRange {
                range: entry_range,
                parent: Some(Box::new(file_sel)),
            };

            // Try to narrow to header, body, or footer.
            if let Some(header) = entry.header() {
                let r = header.syntax().text_range();
                if r.contains(offset) || r.end() == offset {
                    return SelectionRange {
                        range: text_range_to_lsp_range(source_text, r),
                        parent: Some(Box::new(entry_sel)),
                    };
                }
            }

            if let Some(body) = entry.body() {
                let r = body.syntax().text_range();
                if r.contains(offset) || r.end() == offset {
                    return SelectionRange {
                        range: text_range_to_lsp_range(source_text, r),
                        parent: Some(Box::new(entry_sel)),
                    };
                }
            }

            if let Some(footer) = entry.footer() {
                let r = footer.syntax().text_range();
                if r.contains(offset) || r.end() == offset {
                    return SelectionRange {
                        range: text_range_to_lsp_range(source_text, r),
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

    const ENTRY: &str =
        "pkg (1.0-1) unstable; urgency=low\n\n  * Change.\n\n -- T <t@t.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";

    #[test]
    fn test_selection_in_header() {
        let parsed = ChangeLog::parse(ENTRY);
        let ranges = generate_selection_ranges(&parsed, ENTRY, &[Position::new(0, 5)]);
        assert_eq!(ranges.len(), 1);

        let sel = &ranges[0];
        // Innermost: header (includes trailing newline, so ends on line 1)
        assert_eq!(sel.range.start.line, 0);

        // Parent: entry
        let entry_sel = sel.parent.as_ref().unwrap();
        assert_eq!(entry_sel.range.start.line, 0);

        // Grandparent: file
        assert!(entry_sel.parent.as_ref().unwrap().parent.is_none());
    }

    #[test]
    fn test_selection_in_body() {
        let parsed = ChangeLog::parse(ENTRY);
        // Line 2: "  * Change."
        let ranges = generate_selection_ranges(&parsed, ENTRY, &[Position::new(2, 4)]);
        assert_eq!(ranges.len(), 1);

        let sel = &ranges[0];
        // Innermost: body (starts after header)
        assert_eq!(sel.range.start.line, 2);

        // Parent: entry
        let entry_sel = sel.parent.as_ref().unwrap();
        assert_eq!(entry_sel.range.start.line, 0);
    }

    #[test]
    fn test_selection_in_footer() {
        let parsed = ChangeLog::parse(ENTRY);
        // Line 4: " -- T <t@t.com>  ..."
        let ranges = generate_selection_ranges(&parsed, ENTRY, &[Position::new(4, 5)]);
        assert_eq!(ranges.len(), 1);

        let sel = &ranges[0];
        // Innermost: footer
        assert_eq!(sel.range.start.line, 4);

        // Parent: entry
        let entry_sel = sel.parent.as_ref().unwrap();
        assert_eq!(entry_sel.range.start.line, 0);
    }

    #[test]
    fn test_multiple_entries() {
        let text = "\
pkg (2.0-1) unstable; urgency=medium

  * Second release.

 -- A <a@example.com>  Mon, 01 Jan 2025 12:00:00 +0000

pkg (1.0-1) experimental; urgency=low

  * First release.

 -- B <b@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
";
        let parsed = ChangeLog::parse(text);

        // Position in the second entry header
        let ranges = generate_selection_ranges(&parsed, text, &[Position::new(6, 3)]);
        assert_eq!(ranges.len(), 1);

        let sel = &ranges[0];
        // Header of second entry
        assert_eq!(sel.range.start.line, 6);

        // Parent: second entry
        let entry_sel = sel.parent.as_ref().unwrap();
        assert_eq!(entry_sel.range.start.line, 6);

        // Grandparent: file
        let file_sel = entry_sel.parent.as_ref().unwrap();
        assert_eq!(file_sel.range.start.line, 0);
        assert!(file_sel.parent.is_none());
    }

    #[test]
    fn test_empty_changelog() {
        let text = "";
        let parsed = ChangeLog::parse(text);
        let ranges = generate_selection_ranges(&parsed, text, &[Position::new(0, 0)]);
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].parent.is_none());
    }
}
