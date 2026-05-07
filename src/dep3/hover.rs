//! Hover docs for DEP-3 patch headers.

use tower_lsp_server::ls_types::{Hover, Position};

use super::fields::DEP3_FIELDS;
use crate::position::{LineIndex, Source};

/// Hover info for the DEP-3 header at `position`. `header` is the
/// parsed deb822 of the header portion only; `header_end` is where
/// the diff body starts. Returns `None` if the cursor is in the
/// diff body, or on a field name we don't have docs for.
pub fn get_hover(
    header: &deb822_lossless::Deb822,
    header_end: usize,
    src: Source<'_>,
    position: Position,
) -> Option<Hover> {
    if !super::is_in_dep3_header(src, header_end, position) {
        return None;
    }
    let header_text = &src.text[..header_end];
    let header_idx = LineIndex::new(header_text);
    let header_src = Source::new(header_text, &header_idx);
    crate::deb822::hover::get_hover(header, header_src, position, DEP3_FIELDS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, position: Position) -> Option<Hover> {
        let header_end = dep3::lossless::header_end(text);
        let parsed = deb822_lossless::Deb822::parse(&text[..header_end]);
        let idx = LineIndex::new(text);
        get_hover(
            &parsed.tree(),
            header_end,
            Source::new(text, &idx),
            position,
        )
    }

    #[test]
    fn hover_on_known_field_in_header() {
        let hover = run(
            "Author: alice\nForwarded: not-needed\n",
            Position::new(1, 3),
        )
        .expect("hover available");
        match hover.contents {
            tower_lsp_server::ls_types::HoverContents::Markup(m) => {
                assert!(m.value.contains("**Forwarded**"));
                assert!(m.value.contains("not-needed"));
            }
            _ => panic!("Expected markup content"),
        }
    }

    #[test]
    fn hover_in_diff_body_returns_none() {
        // Position on the `---` line.
        assert!(run("Author: alice\n---\n@@ -1 +1 @@\n", Position::new(1, 1)).is_none());
    }

    #[test]
    fn hover_on_unknown_field_returns_none() {
        assert!(run("Author: alice\nX-Custom: y\n", Position::new(1, 3)).is_none());
    }
}
