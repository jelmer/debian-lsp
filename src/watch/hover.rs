use tower_lsp_server::ls_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};

use crate::deb822::completion::{get_cursor_context, CursorContext};

use super::fields::{field_description as get_field_description, linebased_option_description};
use crate::position::Source;

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
    src: Source<'_>,
    position: Position,
) -> Option<Hover> {
    let ctx = get_cursor_context(deb822, src, position)?;

    match ctx {
        CursorContext::FieldKey => {
            let offset = src.try_position_to_offset(position)?;

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

/// Get hover information for a debian/watch v1-4 (line-based) file at the given
/// cursor position.
///
/// Shows the description of the option (e.g. `uversionmangle`) under the cursor,
/// or of the `version=` directive, reusing the shared field/option table.
pub fn get_linebased_hover(
    wf: &debian_watch::linebased::WatchFile,
    src: Source<'_>,
    position: Position,
) -> Option<Hover> {
    use debian_watch::SyntaxKind;

    let offset = src.try_position_to_offset(position)?;
    let token = wf.syntax().token_at_offset(offset).right_biased()?;

    // Only the option/version key name carries documentation.
    if token.kind() != SyntaxKind::KEY {
        return None;
    }
    let parent = token.parent()?;
    match parent.kind() {
        SyntaxKind::OPTION => linebased_option_description(token.text())
            .map(|(canonical, description)| make_hover(canonical, description)),
        SyntaxKind::VERSION => Some(make_hover(
            token.text(),
            super::fields::version_directive_description(),
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hover_on_mode() {
        let text = "Version: 5\n\nSource: https://example.com\nMode: git\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let idx = crate::position::LineIndex::new(text);
        let hover = get_hover(&deb822, Source::new(text, &idx), Position::new(3, 2));
        assert!(hover.is_some());
    }

    #[test]
    fn test_hover_on_unknown_field() {
        let text = "Version: 5\n\nUnknown: value\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let idx = crate::position::LineIndex::new(text);
        let hover = get_hover(&deb822, Source::new(text, &idx), Position::new(2, 3));
        assert!(hover.is_none());
    }

    fn linebased_hover_value(text: &str, position: Position) -> Option<String> {
        let wf = debian_watch::linebased::parse_watch_file(text).tree();
        let idx = crate::position::LineIndex::new(text);
        let hover = get_linebased_hover(&wf, Source::new(text, &idx), position)?;
        match hover.contents {
            HoverContents::Markup(m) => Some(m.value),
            _ => None,
        }
    }

    #[test]
    fn test_linebased_hover_on_option_key() {
        let text = "version=4\nopts=\"uversionmangle=s/-/./\" https://example.org/hello/ hello-(.+)\\.tar\\.gz\n";
        // Cursor on the `uversionmangle` option name.
        let value = linebased_hover_value(text, Position::new(1, 8)).expect("hover");
        let (canonical, description) =
            super::super::fields::linebased_option_description("uversionmangle").unwrap();
        assert_eq!(value, format!("**{}**\n\n{}", canonical, description));
    }

    #[test]
    fn test_linebased_hover_on_version_key() {
        let text = "version=4\nhttps://example.org/hello/ hello-(.+)\\.tar\\.gz\n";
        // Cursor on the `version` keyword.
        let value = linebased_hover_value(text, Position::new(0, 3)).expect("hover");
        assert_eq!(
            value,
            format!(
                "**version**\n\n{}",
                super::super::fields::version_directive_description()
            )
        );
    }

    #[test]
    fn test_linebased_hover_on_unknown_option() {
        let text = "version=4\nopts=\"bogus=1\" https://example.org/hello/ hello-(.+)\\.tar\\.gz\n";
        // Cursor on an unknown option name yields no hover.
        let hover = linebased_hover_value(text, Position::new(1, 8));
        assert_eq!(hover, None);
    }

    #[test]
    fn test_linebased_hover_off_key() {
        let text = "version=4\nhttps://example.org/hello/ hello-(.+)\\.tar\\.gz\n";
        // Cursor on the URL (a VALUE, not a KEY) yields no hover.
        let hover = linebased_hover_value(text, Position::new(1, 5));
        assert_eq!(hover, None);
    }
}
