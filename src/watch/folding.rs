//! Folding range generation for Debian watch files.
//!
//! Supports both deb822 (v5) and line-based (v1-4) watch file formats.

use crate::position::Source;
use tower_lsp_server::ls_types::{FoldingRange, FoldingRangeKind};

/// Generate folding ranges for a watch file.
///
/// For deb822 watch files, each paragraph becomes a foldable region.
/// For line-based watch files, each entry becomes a foldable region.
pub fn generate_folding_ranges(
    parse: &debian_watch::parse::Parse,
    src: Source<'_>,
) -> Vec<FoldingRange> {
    match parse.to_watch_file() {
        debian_watch::parse::ParsedWatchFile::Deb822(wf) => {
            crate::deb822::folding::generate_folding_ranges(wf.as_deb822(), src)
        }
        debian_watch::parse::ParsedWatchFile::LineBased(wf) => {
            generate_linebased_folding_ranges(&wf, src)
        }
    }
}

fn generate_linebased_folding_ranges(
    wf: &debian_watch::linebased::WatchFile,
    src: Source<'_>,
) -> Vec<FoldingRange> {
    wf.entries()
        .filter_map(|entry: debian_watch::linebased::Entry| {
            let range = src.text_range_to_lsp_range(entry.syntax().text_range());
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
    fn test_v4_multiline_entry() {
        let text =
            "version=4\nopts=foo=bar \\\nhttps://example.com .*/foo-(\\d[\\d.]*)/.tar\\.gz\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let idx = crate::position::LineIndex::new(text);
        let ranges = generate_folding_ranges(&parsed, Source::new(text, &idx));

        // The entry with continuation line spans multiple lines
        assert!(!ranges.is_empty());
        assert_eq!(ranges[0].kind, Some(FoldingRangeKind::Region));
    }

    #[test]
    fn test_v4_single_line_entries_excluded() {
        let text = "version=4\nhttps://example.com .*/foo-(\\d[\\d.]*)/.tar\\.gz\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let idx = crate::position::LineIndex::new(text);
        let ranges = generate_folding_ranges(&parsed, Source::new(text, &idx));

        // Single-line entries should not produce folding ranges
        assert_eq!(ranges.len(), 0);
    }

    #[test]
    fn test_v5_deb822_paragraphs() {
        let text = "\
Version: 5

Source: https://github.com/owner/repo/tags
Matching-Pattern: .*/v?(\\d[\\d.]*)/.tar.gz
";
        let parsed = debian_watch::parse::Parse::parse(text);
        let idx = crate::position::LineIndex::new(text);
        let ranges = generate_folding_ranges(&parsed, Source::new(text, &idx));

        // The second paragraph (Source + Matching-Pattern) should be foldable
        assert!(!ranges.is_empty());
    }

    #[test]
    fn test_empty_watch_file() {
        let text = "version=4\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let idx = crate::position::LineIndex::new(text);
        let ranges = generate_folding_ranges(&parsed, Source::new(text, &idx));

        assert_eq!(ranges.len(), 0);
    }
}
