use tower_lsp_server::ls_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};

use crate::deb822::completion::{get_cursor_context, CursorContext};

use super::fields::WATCH_FIELDS;

/// Look up a watch field description by name (case-insensitive).
fn get_field_description(field_name: &str) -> Option<(&'static str, &'static str)> {
    let lowercase = field_name.to_lowercase();
    WATCH_FIELDS
        .iter()
        .find(|f| f.deb822_name.to_lowercase() == lowercase)
        .map(|f| (f.deb822_name, f.description))
}

fn make_hover(field_name: &str, description: &str) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("**{}**\n\n{}", field_name, description),
        }),
        range: None,
    }
}

/// Get hover information for a debian/watch v5 (deb822) file at the given cursor position.
pub fn get_hover(
    deb822: &deb822_lossless::Deb822,
    source_text: &str,
    position: Position,
) -> Option<Hover> {
    let ctx = get_cursor_context(deb822, source_text, position)?;

    match ctx {
        CursorContext::FieldKey => {
            let offset = crate::position::try_position_to_offset(source_text, position)?;

            let entry = deb822
                .paragraphs()
                .flat_map(|p| p.entries().collect::<Vec<_>>())
                .find(|entry| {
                    let r = entry.text_range();
                    r.start() <= offset && offset <= r.end()
                })?;

            let field_name = entry.key()?;
            get_field_description(&field_name)
                .map(|(canonical, description)| make_hover(canonical, description))
        }
        CursorContext::FieldValue { field_name, .. } => get_field_description(&field_name)
            .map(|(canonical, description)| make_hover(canonical, description)),
        CursorContext::StartOfLine => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hover_on_mode() {
        let text = "Version: 5\n\nSource: https://example.com\nMode: git\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let hover = get_hover(&deb822, text, Position::new(3, 2));
        assert!(hover.is_some());
    }

    #[test]
    fn test_hover_on_unknown_field() {
        let text = "Version: 5\n\nUnknown: value\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let hover = get_hover(&deb822, text, Position::new(2, 3));
        assert!(hover.is_none());
    }
}
