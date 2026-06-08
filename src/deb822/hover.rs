use tower_lsp_server::ls_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};

use super::completion::{
    field_description as get_field_description, get_cursor_context, CursorContext, FieldInfo,
};
use crate::position::Source;

/// Get hover information for a deb822 document at the given cursor position.
///
/// Returns a hover with the field name and description when the cursor
/// is on a known field name.
pub fn get_hover(
    deb822: &deb822_lossless::Deb822,
    src: Source<'_>,
    position: Position,
    fields: &[FieldInfo],
) -> Option<Hover> {
    let ctx = get_cursor_context(deb822, src, position)?;

    match ctx {
        CursorContext::FieldKey => get_field_hover_at(deb822, src, position, fields),
        CursorContext::FieldValue { field_name, .. } => get_field_description(fields, &field_name)
            .map(|(canonical, description)| make_hover(canonical, description)),
        CursorContext::StartOfLine => None,
    }
}

/// Find the field name at the cursor position and return hover info.
fn get_field_hover_at(
    deb822: &deb822_lossless::Deb822,
    src: Source<'_>,
    position: Position,
    fields: &[FieldInfo],
) -> Option<Hover> {
    let offset = src.try_position_to_offset(position)?;

    let entry = deb822
        .paragraphs()
        .flat_map(|p| p.entries().collect::<Vec<_>>())
        .find(|entry| {
            let r = entry.text_range();
            r.start() <= offset && offset <= r.end()
        })?;

    let field_name = entry.key()?;
    get_field_description(fields, &field_name)
        .map(|(canonical, description)| make_hover(canonical, description))
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

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_FIELDS: &[FieldInfo] = &[
        FieldInfo::new("Source", "Name of the source package"),
        FieldInfo::new("Package", "Binary package name"),
        FieldInfo::new("Build-Depends", "Build dependencies"),
    ];

    #[test]
    fn test_hover_on_field_key() {
        let text = "Source: test\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let idx = crate::position::LineIndex::new(text);
        let hover = get_hover(
            &deb822,
            Source::new(text, &idx),
            Position::new(0, 2),
            TEST_FIELDS,
        );
        assert!(hover.is_some());
        let hover = hover.unwrap();
        match hover.contents {
            HoverContents::Markup(m) => {
                assert!(m.value.contains("**Source**"));
                assert!(m.value.contains("Name of the source package"));
            }
            _ => panic!("Expected markup content"),
        }
    }

    #[test]
    fn test_hover_on_field_value() {
        let text = "Source: test\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let idx = crate::position::LineIndex::new(text);
        let hover = get_hover(
            &deb822,
            Source::new(text, &idx),
            Position::new(0, 10),
            TEST_FIELDS,
        );
        assert!(hover.is_some());
        let hover = hover.unwrap();
        match hover.contents {
            HoverContents::Markup(m) => {
                assert!(m.value.contains("**Source**"));
            }
            _ => panic!("Expected markup content"),
        }
    }

    #[test]
    fn test_hover_on_unknown_field() {
        let text = "Unknown: test\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let idx = crate::position::LineIndex::new(text);
        let hover = get_hover(
            &deb822,
            Source::new(text, &idx),
            Position::new(0, 2),
            TEST_FIELDS,
        );
        assert!(hover.is_none());
    }

    #[test]
    fn test_hover_case_insensitive() {
        let text = "source: test\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let idx = crate::position::LineIndex::new(text);
        let hover = get_hover(
            &deb822,
            Source::new(text, &idx),
            Position::new(0, 2),
            TEST_FIELDS,
        );
        assert!(hover.is_some());
        let hover = hover.unwrap();
        match hover.contents {
            HoverContents::Markup(m) => {
                // Should show canonical casing
                assert!(m.value.contains("**Source**"));
            }
            _ => panic!("Expected markup content"),
        }
    }

    #[test]
    fn test_hover_on_empty_line() {
        let text = "Source: test\n\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let idx = crate::position::LineIndex::new(text);
        let hover = get_hover(
            &deb822,
            Source::new(text, &idx),
            Position::new(1, 0),
            TEST_FIELDS,
        );
        assert!(hover.is_none());
    }

    #[test]
    fn test_hover_on_start_of_line() {
        let text = "Source: test\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let idx = crate::position::LineIndex::new(text);
        let hover = get_hover(
            &deb822,
            Source::new(text, &idx),
            Position::new(1, 0),
            TEST_FIELDS,
        );
        assert!(hover.is_none());
    }
}
