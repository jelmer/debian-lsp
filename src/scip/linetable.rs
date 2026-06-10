//! Map byte offsets within a document to SCIP `[line, col, line, col]` ranges.
//!
//! SCIP positions are zero-indexed. We emit them using UTF-8 code-unit offsets
//! from the start of the line, which is what [`scip::types::PositionEncoding::UTF8CodeUnitOffsetFromLineStart`]
//! specifies.
//!
//! This is a thin wrapper over [`crate::position::LineIndex`], which owns the
//! line-start table and offset/position conversions shared with the LSP server.

use crate::position::LineIndex;
use crate::scip::symbols;
use scip::types::Occurrence;
use text_size::{TextRange, TextSize};

/// Precomputed table of line-start byte offsets for a single document.
pub struct LineTable {
    index: LineIndex,
}

impl LineTable {
    /// Build a line table for `text`.
    pub fn new(text: &str) -> Self {
        Self {
            index: LineIndex::new(text),
        }
    }

    /// Convert a byte offset into a `(line, col)` pair, both zero-indexed.
    ///
    /// `col` is measured in UTF-8 code units from the start of the line.
    pub fn line_col(&self, offset: u32) -> (i32, i32) {
        let (line, col) = self.index.offset_to_line_col_utf8(TextSize::from(offset));
        (line as i32, col as i32)
    }

    /// Convert a byte range into a SCIP four-element range
    /// `[start_line, start_col, end_line, end_col]`.
    pub fn range(&self, start: u32, end: u32) -> Vec<i32> {
        self.index
            .scip_range(TextRange::new(TextSize::from(start), TextSize::from(end)))
    }

    /// Build a reference occurrence linking the `start..end` email span to its
    /// cross-archive [`symbols::identity`] symbol.
    ///
    /// Every place a person's email appears -- control Maintainer/Uploaders,
    /// DEP-3 patch headers, the changelog footer, DEP-5 Copyright, debcargo
    /// uploaders -- emits this identical occurrence, so "find references" on a
    /// person gathers them all.
    pub fn identity_occurrence(&self, email: &str, start: u32, end: u32) -> Occurrence {
        Occurrence {
            range: self.range(start, end),
            symbol: symbols::identity(email),
            syntax_kind: scip::types::SyntaxKind::IdentifierConstant.into(),
            ..Default::default()
        }
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
