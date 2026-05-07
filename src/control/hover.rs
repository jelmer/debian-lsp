use tower_lsp_server::ls_types::{Hover, Position};

use super::fields::CONTROL_FIELDS;
use crate::position::Source;

/// Get hover information for a debian/control file at the given cursor position.
pub fn get_hover(
    deb822: &deb822_lossless::Deb822,
    src: Source<'_>,
    position: Position,
) -> Option<Hover> {
    crate::deb822::hover::get_hover(deb822, src, position, CONTROL_FIELDS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;

    #[test]
    fn test_hover_on_build_depends() {
        let text = "Source: test\nBuild-Depends: debhelper\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let idx = LineIndex::new(text);

        let hover = get_hover(&deb822, Source::new(text, &idx), Position::new(1, 5));
        assert!(hover.is_some());
    }

    #[test]
    fn test_hover_on_unknown_field() {
        let text = "Source: test\nX-Custom: value\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let idx = LineIndex::new(text);

        let hover = get_hover(&deb822, Source::new(text, &idx), Position::new(1, 3));
        assert!(hover.is_none());
    }
}
