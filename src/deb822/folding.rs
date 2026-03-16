//! Generic folding range generation for deb822 files.
//!
//! Each paragraph in a deb822 file becomes a foldable region.

use deb822_lossless::Deb822;
use tower_lsp_server::ls_types::{FoldingRange, FoldingRangeKind};

use crate::position::text_range_to_lsp_range;

/// Generate folding ranges for a deb822 document.
///
/// Each paragraph that spans more than one line produces a `Region` folding
/// range. Single-line paragraphs are omitted because there is nothing to fold.
pub fn generate_folding_ranges(deb822: &Deb822, source_text: &str) -> Vec<FoldingRange> {
    deb822
        .paragraphs()
        .filter_map(|para| {
            let range = text_range_to_lsp_range(source_text, para.text_range());
            // When the range ends at column 0, the actual content ends on the
            // previous line (the trailing newline pushed us to the next line).
            let end_line = if range.end.character == 0 && range.end.line > range.start.line {
                range.end.line - 1
            } else {
                range.end.line
            };
            if range.start.line == end_line {
                return None;
            }
            Some(FoldingRange {
                start_line: range.start.line,
                start_character: None,
                end_line,
                end_character: None,
                kind: Some(FoldingRangeKind::Region),
                collapsed_text: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_paragraph() {
        let text = "Source: foo\nMaintainer: Test <test@example.com>\n";
        let parsed = Deb822::parse(text);
        let deb822 = parsed.tree();
        let ranges = generate_folding_ranges(&deb822, text);

        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start_line, 0);
        assert_eq!(ranges[0].end_line, 1);
    }

    #[test]
    fn test_multiple_paragraphs() {
        let text = "\
Source: foo
Maintainer: Test <test@example.com>

Package: foo
Architecture: any
Description: A test package
";
        let parsed = Deb822::parse(text);
        let deb822 = parsed.tree();
        let ranges = generate_folding_ranges(&deb822, text);

        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].start_line, 0);
        assert_eq!(ranges[0].end_line, 1);
        assert_eq!(ranges[1].start_line, 3);
        assert_eq!(ranges[1].end_line, 5);
    }

    #[test]
    fn test_empty_file() {
        let text = "";
        let parsed = Deb822::parse(text);
        let deb822 = parsed.tree();
        let ranges = generate_folding_ranges(&deb822, text);

        assert_eq!(ranges.len(), 0);
    }

    #[test]
    fn test_single_line_paragraph_excluded() {
        let text = "Source: foo\n";
        let parsed = Deb822::parse(text);
        let deb822 = parsed.tree();
        let ranges = generate_folding_ranges(&deb822, text);

        assert_eq!(ranges.len(), 0);
    }

    #[test]
    fn test_folding_kind_is_region() {
        let text = "Source: foo\nMaintainer: Test <test@example.com>\n";
        let parsed = Deb822::parse(text);
        let deb822 = parsed.tree();
        let ranges = generate_folding_ranges(&deb822, text);

        assert_eq!(ranges[0].kind, Some(FoldingRangeKind::Region));
    }
}
