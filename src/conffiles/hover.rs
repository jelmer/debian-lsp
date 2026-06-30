use tower_lsp_server::ls_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};

use super::REMOVE_ON_UPGRADE;
use crate::position::Source;

/// Get hover information for a debian/conffiles file at the given cursor position.
pub fn get_hover(src: Source<'_>, position: Position) -> Option<Hover> {
    let current_line = src
        .text
        .lines()
        .nth(position.line as usize)
        .unwrap_or("")
        .trim();

    if current_line.is_empty() || current_line.starts_with('#') {
        return None;
    }

    let flag = REMOVE_ON_UPGRADE;
    let flag_desc = "Remove this file when the package is upgraded";

    // Cursor on remove-on-upgrade flag
    if current_line.starts_with(flag) {
        let char_pos = position.character as usize;
        if char_pos <= flag.len() {
            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: format!("**{}**\n\n{}", flag, flag_desc),
                }),
                range: None,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;

    fn hover(text: &str, line: u32, col: u32) -> Option<Hover> {
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        get_hover(src, Position::new(line, col))
    }

    fn md(h: &Hover) -> &str {
        match &h.contents {
            HoverContents::Markup(m) => &m.value,
            _ => panic!("expected markup"),
        }
    }

    #[test]
    fn test_hover_on_remove_on_upgrade_flag() {
        let h = hover("remove-on-upgrade /etc/myapp/old.conf\n", 0, 5).unwrap();
        assert!(md(&h).contains("remove-on-upgrade"));
        assert!(md(&h).contains("Remove"));
    }

    #[test]
    fn test_hover_on_path_is_none() {
        assert!(hover("/etc/myapp/config.conf\n", 0, 5).is_none());
    }

    #[test]
    fn test_hover_on_path_after_flag_return_none() {
        assert!(hover("remove-on-upgrade /etc/myapp/old.conf\n", 0, 20).is_none());
    }

    #[test]
    fn test_hover_empty_line_returns_none() {
        assert!(hover("\n", 0, 0).is_none());
    }

    #[test]
    fn test_hover_comment_returns_none() {
        assert!(hover("# this is a comment\n", 0, 5).is_none());
    }
}
