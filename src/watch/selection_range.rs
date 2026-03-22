//! Selection range generation for Debian watch files.
//!
//! Supports both deb822 (v5) and line-based (v1-4) watch file formats.
//! For deb822: value → entry → paragraph → file (via generic deb822 support).
//! For line-based: entry → file.

use text_size::TextSize;
use tower_lsp_server::ls_types::{Position, Range, SelectionRange};

use crate::position::{offset_to_position, text_range_to_lsp_range, try_position_to_offset};

/// Generate selection ranges for a watch file.
pub fn generate_selection_ranges(
    parse: &debian_watch::parse::Parse,
    source_text: &str,
    positions: &[Position],
) -> Vec<SelectionRange> {
    match parse.to_watch_file() {
        debian_watch::parse::ParsedWatchFile::Deb822(wf) => {
            crate::deb822::selection_range::generate_selection_ranges(
                wf.as_deb822(),
                source_text,
                positions,
            )
        }
        debian_watch::parse::ParsedWatchFile::LineBased(wf) => {
            generate_linebased_selection_ranges(&wf, source_text, positions)
        }
    }
}

fn generate_linebased_selection_ranges(
    wf: &debian_watch::linebased::WatchFile,
    source_text: &str,
    positions: &[Position],
) -> Vec<SelectionRange> {
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

            let Some(entry) = wf.entries().find(|e| {
                let r = e.syntax().text_range();
                r.contains(offset) || r.end() == offset
            }) else {
                return file_sel;
            };

            SelectionRange {
                range: text_range_to_lsp_range(source_text, entry.syntax().text_range()),
                parent: Some(Box::new(file_sel)),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v4_selection_in_entry() {
        let text = "version=4\nhttps://example.com .*/foo-(\\d[\\d.]*)/.tar\\.gz\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let ranges = generate_selection_ranges(&parsed, text, &[Position::new(1, 5)]);
        assert_eq!(ranges.len(), 1);

        let sel = &ranges[0];
        // Entry range
        assert_eq!(sel.range.start.line, 1);

        // Parent: file
        let file_sel = sel.parent.as_ref().unwrap();
        assert_eq!(file_sel.range.start.line, 0);
        assert!(file_sel.parent.is_none());
    }

    #[test]
    fn test_v4_selection_in_version_line() {
        let text = "version=4\nhttps://example.com .*/foo-(\\d[\\d.]*)/.tar\\.gz\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        // Position on the "version=4" line — not inside any entry
        let ranges = generate_selection_ranges(&parsed, text, &[Position::new(0, 3)]);
        assert_eq!(ranges.len(), 1);

        // Should fall back to file range
        assert!(ranges[0].parent.is_none());
    }

    #[test]
    fn test_v5_deb822_selection() {
        let text = "\
Version: 5

Source: https://github.com/owner/repo/tags
Matching-Pattern: .*/v?(\\d[\\d.]*)/.tar.gz
";
        let parsed = debian_watch::parse::Parse::parse(text);
        // Position in "Source" value
        let ranges = generate_selection_ranges(&parsed, text, &[Position::new(2, 10)]);
        assert_eq!(ranges.len(), 1);

        // Should have value → entry → paragraph → file hierarchy
        let sel = &ranges[0];
        assert!(sel.parent.is_some());
    }

    #[test]
    fn test_empty_watch_file() {
        let text = "version=4\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let ranges = generate_selection_ranges(&parsed, text, &[Position::new(0, 3)]);
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].parent.is_none());
    }
}
