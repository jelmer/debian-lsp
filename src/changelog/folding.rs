//! Folding range generation for Debian changelog files.
//!
//! Each changelog entry becomes a foldable region.

use debian_changelog::{ChangeLog, Parse};
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{FoldingRange, FoldingRangeKind};

use crate::position::text_range_to_lsp_range;

/// Generate folding ranges for a changelog file.
///
/// Each entry that spans more than one line produces a `Region` folding range.
pub fn generate_folding_ranges(parse: &Parse<ChangeLog>, source_text: &str) -> Vec<FoldingRange> {
    let changelog = parse.tree();

    changelog
        .iter()
        .filter_map(|entry| {
            let range = text_range_to_lsp_range(source_text, entry.syntax().text_range());
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
    fn test_single_entry() {
        let text = "pkg (1.0-1) unstable; urgency=low\n\n  * Change.\n\n -- T <t@t.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = ChangeLog::parse(text);
        let ranges = generate_folding_ranges(&parsed, text);

        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start_line, 0);
        assert_eq!(ranges[0].end_line, 4);
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
        let ranges = generate_folding_ranges(&parsed, text);

        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].start_line, 0);
        assert_eq!(ranges[0].end_line, 4);
        assert_eq!(ranges[1].start_line, 6);
        assert_eq!(ranges[1].end_line, 10);
    }

    #[test]
    fn test_empty_changelog() {
        let text = "";
        let parsed = ChangeLog::parse(text);
        let ranges = generate_folding_ranges(&parsed, text);

        assert_eq!(ranges.len(), 0);
    }

    #[test]
    fn test_ranges_do_not_overlap() {
        let text = "\
pkg (2.0-1) unstable; urgency=medium

  * Second.

 -- A <a@a.com>  Mon, 01 Jan 2025 12:00:00 +0000

pkg (1.0-1) unstable; urgency=low

  * First.

 -- B <b@b.com>  Mon, 01 Jan 2024 12:00:00 +0000
";
        let parsed = ChangeLog::parse(text);
        let ranges = generate_folding_ranges(&parsed, text);

        assert_eq!(ranges.len(), 2);
        assert!(ranges[0].end_line < ranges[1].start_line);
    }

    #[test]
    fn test_folding_kind_is_region() {
        let text = "pkg (1.0-1) unstable; urgency=low\n\n  * Change.\n\n -- T <t@t.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = ChangeLog::parse(text);
        let ranges = generate_folding_ranges(&parsed, text);

        assert_eq!(ranges[0].kind, Some(FoldingRangeKind::Region));
    }
}
