use tower_lsp_server::ls_types::{Hover, Position};

use super::fields::COPYRIGHT_FIELDS;

/// Get hover information for a debian/copyright file at the given cursor position.
pub fn get_hover(
    deb822: &deb822_lossless::Deb822,
    source_text: &str,
    position: Position,
) -> Option<Hover> {
    crate::deb822::hover::get_hover(deb822, source_text, position, COPYRIGHT_FIELDS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hover_on_format() {
        let text = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let hover = get_hover(&deb822, text, Position::new(0, 3));
        assert!(hover.is_some());
        let hover = hover.unwrap();
        match hover.contents {
            tower_lsp_server::ls_types::HoverContents::Markup(m) => {
                assert!(m.value.contains("**Format**"));
                assert!(m.value.contains("format specification"));
            }
            _ => panic!("Expected markup content"),
        }
    }

    #[test]
    fn test_hover_on_files() {
        let text = "Format: https://example.com\n\nFiles: *\nCopyright: 2024 Test\nLicense: MIT\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let hover = get_hover(&deb822, text, Position::new(2, 2));
        assert!(hover.is_some());
        let hover = hover.unwrap();
        match hover.contents {
            tower_lsp_server::ls_types::HoverContents::Markup(m) => {
                assert!(m.value.contains("**Files**"));
                assert!(m.value.contains("fnmatch"));
            }
            _ => panic!("Expected markup content"),
        }
    }

    #[test]
    fn test_hover_on_license() {
        let text = "Format: https://example.com\n\nFiles: *\nCopyright: 2024 Test\nLicense: MIT\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let hover = get_hover(&deb822, text, Position::new(4, 3));
        assert!(hover.is_some());
        let hover = hover.unwrap();
        match hover.contents {
            tower_lsp_server::ls_types::HoverContents::Markup(m) => {
                assert!(m.value.contains("**License**"));
                assert!(m.value.contains("synopsis"));
            }
            _ => panic!("Expected markup content"),
        }
    }

    #[test]
    fn test_hover_on_copyright() {
        let text = "Format: https://example.com\n\nFiles: *\nCopyright: 2024 Test\nLicense: MIT\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let hover = get_hover(&deb822, text, Position::new(3, 3));
        assert!(hover.is_some());
        let hover = hover.unwrap();
        match hover.contents {
            tower_lsp_server::ls_types::HoverContents::Markup(m) => {
                assert!(m.value.contains("**Copyright**"));
                assert!(m.value.contains("copyright statement"));
            }
            _ => panic!("Expected markup content"),
        }
    }

    #[test]
    fn test_hover_unknown_field() {
        let text = "Format: https://example.com\nUnknown: value\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let hover = get_hover(&deb822, text, Position::new(1, 3));
        assert!(hover.is_none());
    }
}
