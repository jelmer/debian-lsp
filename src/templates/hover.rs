use tower_lsp_server::ls_types::{Hover, Position};

use super::fields::TEMPLATES_FIELDS;
use crate::position::{LineIndex, Source};

/// Hover info for the debconf template field at `position`.
pub fn get_hover(text: &str, position: Position) -> Option<Hover> {
    let deb822 = deb822_lossless::Deb822::parse(text).tree();
    let idx = LineIndex::new(text);
    let src = Source::new(text, &idx);
    crate::deb822::hover::get_hover(&deb822, src, position, TEMPLATES_FIELDS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp_server::ls_types::HoverContents;

    #[test]
    fn hover_on_known_field() {
        let hover = get_hover("Template: foo/bar\nType: select\n", Position::new(1, 2))
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
        assert!(get_hover("Description-fr.UTF-8: bonjour\n", Position::new(0, 3)).is_none());
    }
}
