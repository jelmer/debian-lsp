//! Position helpers for DEP-3 header regions.

use tower_lsp_server::ls_types::Position;

use crate::position::try_position_to_offset;

/// Return true if `position` falls inside the DEP-3 header portion of
/// `text` — i.e. before the first `---` / `diff ` / `Index:` line.
/// Positions on or after the diff body return `false` so LSP features
/// don't reach into the unified-diff territory that diff-lsp owns.
pub fn is_in_dep3_header(text: &str, position: Position) -> bool {
    let header_end = dep3::lossless::header_end(text);
    let Some(offset) = try_position_to_offset(text, position) else {
        return false;
    };
    let offset: usize = offset.into();
    offset < header_end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_in_header_returns_true() {
        let text = "Author: alice\nDescription: bla\n---\n@@ -1 +1 @@\n";
        assert!(is_in_dep3_header(text, Position::new(0, 0)));
        assert!(is_in_dep3_header(text, Position::new(1, 5)));
    }

    #[test]
    fn cursor_in_diff_body_returns_false() {
        let text = "Author: alice\n---\n@@ -1 +1 @@\n-x\n+y\n";
        // Line 2 is `---`, line 3 is the hunk header.
        assert!(!is_in_dep3_header(text, Position::new(2, 0)));
        assert!(!is_in_dep3_header(text, Position::new(3, 1)));
    }

    #[test]
    fn cursor_at_diff_marker_line_returns_false() {
        let text = "Author: alice\n---\n";
        // Line 1 is `---` (the boundary); positions on it are body.
        assert!(!is_in_dep3_header(text, Position::new(1, 0)));
    }

    #[test]
    fn header_only_file_is_all_header() {
        let text = "Author: alice\nDescription: bla\n";
        assert!(is_in_dep3_header(text, Position::new(0, 0)));
        assert!(is_in_dep3_header(text, Position::new(1, 5)));
    }
}
