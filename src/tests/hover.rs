use tower_lsp_server::ls_types::{Hover, Position};

use super::fields::TESTS_FIELDS;
use crate::position::Source;

/// Get hover information for a debian/tests/control file at the given cursor position.
pub fn get_hover(
    deb822: &deb822_lossless::Deb822,
    src: Source<'_>,
    position: Position,
) -> Option<Hover> {
    crate::deb822::hover::get_hover(deb822, src, position, TESTS_FIELDS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;

    #[test]
    fn test_hover_on_tests_field() {
        let text = "Tests: smoke\nDepends: @\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let idx = LineIndex::new(text);
        let hover = get_hover(&deb822, Source::new(text, &idx), Position::new(0, 2));
        assert!(hover.is_some());
        let contents = match hover.unwrap().contents {
            tower_lsp_server::ls_types::HoverContents::Markup(m) => m.value,
            _ => panic!("Expected markup content"),
        };
        assert!(contents.contains("Test script names in the test directory"));
    }

    #[test]
    fn test_hover_on_restrictions_field() {
        let text = "Tests: smoke\nRestrictions: needs-root\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let idx = LineIndex::new(text);
        let hover = get_hover(&deb822, Source::new(text, &idx), Position::new(1, 5));
        assert!(hover.is_some());
        let contents = match hover.unwrap().contents {
            tower_lsp_server::ls_types::HoverContents::Markup(m) => m.value,
            _ => panic!("Expected markup content"),
        };
        assert!(contents.contains("Restrictions on how the test can be run"));
    }

    #[test]
    fn test_hover_on_unknown_field() {
        let text = "Tests: smoke\nX-Custom: value\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let idx = LineIndex::new(text);
        let hover = get_hover(&deb822, Source::new(text, &idx), Position::new(1, 3));
        assert!(hover.is_none());
    }
}
