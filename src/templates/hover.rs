use tower_lsp_server::ls_types::{Hover, Position};

use super::fields::TEMPLATES_FIELDS;
use crate::position::Source;

/// Hover info for the debconf template field at `position`.
pub fn get_hover(
    deb822: &deb822_lossless::Deb822,
    src: Source<'_>,
    position: Position,
) -> Option<Hover> {
    crate::deb822::hover::get_hover(deb822, src, position, TEMPLATES_FIELDS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;
    use tower_lsp_server::ls_types::HoverContents;

    fn hover_at(text: &str, position: Position) -> Option<Hover> {
        let deb822 = deb822_lossless::Deb822::parse(text).tree();
        let idx = LineIndex::new(text);
        get_hover(&deb822, Source::new(text, &idx), position)
    }

    #[test]
    fn hover_on_known_field() {
        let hover = hover_at("Template: foo/bar\nType: select\n", Position::new(1, 2))
            .expect("hover available");
        match hover.contents {
            HoverContents::Markup(m) => {
                assert!(m.value.contains("**Type**"));
                assert!(m.value.contains("Widget type"));
            }
            _ => panic!("Expected markup content"),
        }
    }

    #[test]
    fn hover_on_unknown_field_returns_none() {
        assert!(hover_at("Description-fr.UTF-8: bonjour\n", Position::new(0, 3)).is_none());
    }
}
