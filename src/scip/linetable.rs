//! Map byte offsets within a document to SCIP `[line, col, line, col]` ranges.
//!
//! SCIP positions are zero-indexed. We emit them using UTF-8 code-unit offsets
//! from the start of the line, which is what [`scip::types::PositionEncoding::UTF8CodeUnitOffsetFromLineStart`]
//! specifies.

/// Precomputed table of line-start byte offsets for a single document.
///
/// Built once per file, then queried to convert byte offsets to `(line, col)`
/// pairs in `O(log n)` time.
pub struct LineTable {
    /// Byte offset of the start of each line. Always begins with `0`.
    starts: Vec<u32>,
}

impl LineTable {
    /// Build a line table for `text`.
    pub fn new(text: &str) -> Self {
        let mut starts = Vec::with_capacity(text.len() / 40 + 1);
        starts.push(0);
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                starts.push((i + 1) as u32);
            }
        }
        Self { starts }
    }

    /// Convert a byte offset into a `(line, col)` pair, both zero-indexed.
    ///
    /// `col` is measured in UTF-8 code units from the start of the line.
    pub fn line_col(&self, offset: u32) -> (i32, i32) {
        let idx = match self.starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let line_start = self.starts[idx];
        (idx as i32, (offset - line_start) as i32)
    }

    /// Convert a byte range into a SCIP four-element range
    /// `[start_line, start_col, end_line, end_col]`.
    pub fn range(&self, start: u32, end: u32) -> Vec<i32> {
        let (sl, sc) = self.line_col(start);
        let (el, ec) = self.line_col(end);
        vec![sl, sc, el, ec]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line() {
        let t = LineTable::new("hello world");
        assert_eq!(t.line_col(0), (0, 0));
        assert_eq!(t.line_col(6), (0, 6));
        assert_eq!(t.range(0, 5), vec![0, 0, 0, 5]);
    }

    #[test]
    fn multi_line() {
        let t = LineTable::new("ab\ncde\nf");
        assert_eq!(t.line_col(0), (0, 0));
        assert_eq!(t.line_col(2), (0, 2));
        assert_eq!(t.line_col(3), (1, 0));
        assert_eq!(t.line_col(6), (1, 3));
        assert_eq!(t.line_col(7), (2, 0));
        assert_eq!(t.range(3, 6), vec![1, 0, 1, 3]);
    }
}
