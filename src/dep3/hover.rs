//! Hover docs for DEP-3 patch headers.

use tower_lsp_server::ls_types::{Hover, Position};

use super::fields::DEP3_FIELDS;

/// Hover info for the DEP-3 header at `position`. Returns `None` if
/// the cursor is in the diff body, or on a field name we don't have
/// docs for.
pub fn get_hover(source_text: &str, position: Position) -> Option<Hover> {
    if !super::is_in_dep3_header(source_text, position) {
        return None;
    }
    let header_end = dep3::lossless::header_end(source_text);
    let header_text = &source_text[..header_end];
    let parsed = deb822_lossless::Deb822::parse(header_text);
    let deb822 = parsed.tree();
    crate::deb822::hover::get_hover(&deb822, header_text, position, DEP3_FIELDS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_on_known_field_in_header() {
        let text = "Author: alice\nForwarded: not-needed\n";
        let hover = get_hover(text, Position::new(1, 3)).expect("hover available");
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
        let text = "Author: alice\n---\n@@ -1 +1 @@\n";
        // Position on the `---` line.
        assert!(get_hover(text, Position::new(1, 1)).is_none());
    }

    #[test]
    fn hover_on_unknown_field_returns_none() {
        let text = "Author: alice\nX-Custom: y\n";
        assert!(get_hover(text, Position::new(1, 3)).is_none());
    }
}
