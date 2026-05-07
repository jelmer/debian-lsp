use text_size::{TextRange, TextSize};
use tower_lsp_server::ls_types::{Position, Range};

/// Return the UTF-16 code unit length of a string.
pub fn utf16_len(s: &str) -> u32 {
    s.chars().map(|c| c.len_utf16() as u32).sum()
}

/// Pre-computed byte offsets where each line starts.
///
/// Building it walks the buffer once (O(N)). After that, mapping a
/// byte offset to its `(line, byte_in_line)` is an O(log N) binary
/// search; computing the UTF-16 column from there is O(line length).
/// Without the index every position conversion is a linear scan from
/// the start of the buffer — for a 100KB changelog that's tens of
/// MB of byte-walking per LSP request when many ranges need
/// conversion.
///
/// `Arc<LineIndex>` is salsa-cached per buffer (via
/// [`crate::workspace::Workspace::get_line_index`]) so all consumers
/// in a single LSP request share one index.
#[derive(Debug, PartialEq, Eq)]
pub struct LineIndex {
    /// Byte offset where each line starts. `line_starts[0]` is always
    /// 0; `line_starts[N]` for N > 0 is the byte after the Nth `\n`.
    line_starts: Vec<TextSize>,
    /// Total byte length of the source. Stored so out-of-range
    /// positions can be detected without re-querying the source.
    text_len: TextSize,
}

impl LineIndex {
    /// Build a line index from `text` in a single linear scan.
    pub fn new(text: &str) -> Self {
        let mut line_starts = Vec::with_capacity(text.bytes().filter(|&b| b == b'\n').count() + 1);
        line_starts.push(TextSize::from(0));
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(TextSize::try_from(i + 1).unwrap());
            }
        }
        Self {
            line_starts,
            text_len: TextSize::try_from(text.len()).unwrap(),
        }
    }

    /// Convert a byte offset to an LSP `Position`. `text` must be
    /// the same buffer the index was built from.
    pub fn offset_to_position(&self, text: &str, offset: TextSize) -> Position {
        let offset = offset.min(self.text_len);
        let line = match self.line_starts.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let line_start: usize = self.line_starts[line].into();
        // Walk only the part of the line up to the offset to count
        // UTF-16 code units. Lines are typically short, so this is
        // effectively O(1).
        let line_text = &text[line_start..usize::from(offset)];
        let utf16_col: u32 = line_text.chars().map(|c| c.len_utf16() as u32).sum();
        Position {
            line: line as u32,
            character: utf16_col,
        }
    }

    /// Convert an LSP `Position` to a byte offset. Returns `None`
    /// when the position is past the end of the buffer.
    pub fn try_position_to_offset(&self, text: &str, position: Position) -> Option<TextSize> {
        let line = position.line as usize;
        if line >= self.line_starts.len() {
            return None;
        }
        let line_start: usize = self.line_starts[line].into();
        let line_end: usize = self
            .line_starts
            .get(line + 1)
            .map(|&s| usize::from(s))
            .unwrap_or_else(|| usize::from(self.text_len));
        // Strip the trailing newline (if any) so columns past the
        // last visible character map to end-of-line content, not the
        // newline byte.
        let content_end =
            if line_end > line_start && text.as_bytes().get(line_end - 1) == Some(&b'\n') {
                line_end - 1
            } else {
                line_end
            };
        let line_text = &text[line_start..content_end];

        let mut utf16_col: u32 = 0;
        for (rel_byte, ch) in line_text.char_indices() {
            if utf16_col >= position.character {
                return TextSize::try_from(line_start + rel_byte).ok();
            }
            utf16_col += ch.len_utf16() as u32;
        }
        if utf16_col >= position.character || position.character == utf16_col {
            return TextSize::try_from(content_end).ok();
        }
        None
    }

    /// Convert a `TextRange` to an LSP `Range`.
    pub fn text_range_to_lsp_range(&self, text: &str, range: TextRange) -> Range {
        Range {
            start: self.offset_to_position(text, range.start()),
            end: self.offset_to_position(text, range.end()),
        }
    }

    /// Convert an LSP `Range` to a `TextRange`. Returns `None` when
    /// either endpoint is past the end of the buffer.
    pub fn try_lsp_range_to_text_range(&self, text: &str, range: &Range) -> Option<TextRange> {
        let start = self.try_position_to_offset(text, range.start)?;
        let end = self.try_position_to_offset(text, range.end)?;
        Some(TextRange::new(start, end))
    }
}

/// Read-only view over a buffer plus its line index.
///
/// Bundles `&str` (the buffer text) with `&LineIndex` (the
/// pre-computed line-start offsets) so call chains that need to
/// convert byte offsets to LSP positions don't have to thread two
/// parameters everywhere. Construct via
/// [`crate::workspace::Workspace::source`] or directly with
/// [`Source::new`].
#[derive(Clone, Copy)]
pub struct Source<'a> {
    /// The buffer text.
    pub text: &'a str,
    /// Pre-computed line index for `text`.
    pub idx: &'a LineIndex,
}

impl<'a> Source<'a> {
    /// Bundle `text` with its line index.
    pub fn new(text: &'a str, idx: &'a LineIndex) -> Self {
        Self { text, idx }
    }

    /// Convert a byte offset to an LSP `Position`.
    pub fn offset_to_position(&self, offset: TextSize) -> Position {
        self.idx.offset_to_position(self.text, offset)
    }

    /// Convert an LSP `Position` to a byte offset, or `None` when
    /// out of range.
    pub fn try_position_to_offset(&self, position: Position) -> Option<TextSize> {
        self.idx.try_position_to_offset(self.text, position)
    }

    /// Convert a `TextRange` to an LSP `Range`.
    pub fn text_range_to_lsp_range(&self, range: TextRange) -> Range {
        self.idx.text_range_to_lsp_range(self.text, range)
    }

    /// Convert an LSP `Range` to a `TextRange`, or `None` when out
    /// of range.
    pub fn try_lsp_range_to_text_range(&self, range: &Range) -> Option<TextRange> {
        self.idx.try_lsp_range_to_text_range(self.text, range)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx(text: &str) -> LineIndex {
        LineIndex::new(text)
    }

    #[test]
    fn test_try_position_to_offset_multiline_value_end() {
        let text = "Source: test\nSection: py\n";
        let offset = idx(text)
            .try_position_to_offset(text, Position::new(1, 11))
            .unwrap();
        assert_eq!(offset, TextSize::from(24u32));
    }

    #[test]
    fn test_try_position_to_offset_returns_none_for_out_of_range_character() {
        let text = "Source: test\nSection: py\n";
        let offset = idx(text).try_position_to_offset(text, Position::new(1, 99));
        assert!(offset.is_none());
    }

    #[test]
    fn test_try_position_to_offset_returns_none_for_out_of_range_line() {
        let text = "Source: test\n";
        let offset = idx(text).try_position_to_offset(text, Position::new(5, 0));
        assert!(offset.is_none());
    }

    #[test]
    fn test_try_lsp_range_to_text_range_valid() {
        let text = "Source: test\nSection: py\n";
        let range = Range::new(Position::new(1, 0), Position::new(1, 7));
        let text_range = idx(text).try_lsp_range_to_text_range(text, &range).unwrap();
        assert_eq!(text_range.start(), TextSize::from(13u32));
        assert_eq!(text_range.end(), TextSize::from(20u32));
    }

    #[test]
    fn test_try_lsp_range_to_text_range_invalid_returns_none() {
        let text = "Source: test\n";
        let range = Range::new(Position::new(10, 0), Position::new(10, 1));
        assert!(idx(text)
            .try_lsp_range_to_text_range(text, &range)
            .is_none());
    }

    #[test]
    fn test_offset_to_position_with_multibyte_chars() {
        // 'ĳ' is U+0133: 2 bytes in UTF-8, 1 code unit in UTF-16
        let text = "Vernooĳ rest";
        let i = idx(text);
        // 'V' at byte 0 -> col 0
        assert_eq!(
            i.offset_to_position(text, TextSize::from(0u32)),
            Position::new(0, 0)
        );
        // 'ĳ' starts at byte 6, col 6
        assert_eq!(
            i.offset_to_position(text, TextSize::from(6u32)),
            Position::new(0, 6)
        );
        // ' ' after 'ĳ' is at byte 8, but UTF-16 col 7
        assert_eq!(
            i.offset_to_position(text, TextSize::from(8u32)),
            Position::new(0, 7)
        );
        // 'r' of "rest" at byte 9, UTF-16 col 8
        assert_eq!(
            i.offset_to_position(text, TextSize::from(9u32)),
            Position::new(0, 8)
        );
    }

    #[test]
    fn test_try_position_to_offset_with_multibyte_chars() {
        let text = "Vernooĳ rest";
        let i = idx(text);
        // col 7 in UTF-16 -> byte 8 (the space after 'ĳ')
        let offset = i.try_position_to_offset(text, Position::new(0, 7)).unwrap();
        assert_eq!(offset, TextSize::from(8u32));
        // col 8 in UTF-16 -> byte 9 ('r')
        let offset = i.try_position_to_offset(text, Position::new(0, 8)).unwrap();
        assert_eq!(offset, TextSize::from(9u32));
    }

    #[test]
    fn test_utf16_len() {
        assert_eq!(utf16_len("hello"), 5);
        assert_eq!(utf16_len("Vernooĳ"), 7); // ĳ is 1 UTF-16 code unit
        assert_eq!(utf16_len(""), 0);
        // Emoji 😀 (U+1F600) is 2 UTF-16 code units (surrogate pair)
        assert_eq!(utf16_len("😀"), 2);
    }

    #[test]
    fn line_index_handles_empty_buffer() {
        let i = LineIndex::new("");
        assert_eq!(
            i.offset_to_position("", TextSize::from(0u32)),
            Position::new(0, 0)
        );
        assert!(i.try_position_to_offset("", Position::new(1, 0)).is_none());
    }

    #[test]
    fn line_index_round_trips_offsets() {
        // For every byte boundary in a few representative buffers,
        // `offset → position → offset` should round-trip.
        for text in [
            "",
            "no newline",
            "one\nline",
            "trailing newline\n",
            "Source: test\nSection: py\n",
            "Vernooĳ\n  middle\n",
        ] {
            let i = LineIndex::new(text);
            for byte_off in 0..=text.len() {
                if !text.is_char_boundary(byte_off) {
                    continue;
                }
                let off = TextSize::try_from(byte_off).unwrap();
                let pos = i.offset_to_position(text, off);
                let back = i
                    .try_position_to_offset(text, pos)
                    .expect("position from offset_to_position must round-trip");
                // If the offset lands on a newline, the position is
                // really the end-of-line-content for the previous
                // line, so try_position_to_offset returns the start
                // of the newline, not after it. Allow that one case.
                if back != off {
                    let was_at_newline =
                        text.as_bytes().get(byte_off.saturating_sub(1)) == Some(&b'\n');
                    assert!(
                        was_at_newline,
                        "round-trip mismatch in {:?}: off={} pos={:?} back={}",
                        text,
                        byte_off,
                        pos,
                        usize::from(back)
                    );
                }
            }
        }
    }
}
