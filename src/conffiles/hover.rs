use std::path::Path;
use tower_lsp_server::ls_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};

use super::fields::CONFFILES_FLAGS;
use crate::position::Source;

/// Get hover information for a debian/conffiles file at the given cursor position.
pub fn get_hover(src: Source<'_>, position: Position, debian_dir: Option<&Path>) -> Option<Hover> {
    let current_line = src
        .text
        .lines()
        .nth(position.line as usize)
        .unwrap_or("")
        .trim();

    if current_line.is_empty() {
        return None;
    }

    // Cursor on remove-on-upgrade flag
    let (flag, flag_desc) = CONFFILES_FLAGS[0];
    if current_line.starts_with(flag) {
        let flag_end = flag.len();
        let char_pos = position.character as usize;
        if char_pos <= flag_end {
            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: format!("**{}**\n\n{}", flag, flag_desc),
                }),
                range: None,
            });
        }
    }

    // Extract the path from the line
    let path_str = if let Some(rest) = current_line.strip_prefix(&format!("{} ", flag)) {
        rest.trim()
    } else {
        current_line
    };

    if !path_str.starts_with('/') {
        return None;
    }

    // Check if file exists in staging
    let rel = path_str.trim_start_matches('/');

    let exists = debian_dir.map_or(false, |d| {
        std::fs::read_dir(d)
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| entry.path().join(rel).exists())
    });

    let value = if exists {
        format!(
            "**{}**\n\nConffile - found in debhelper staging directory",
            path_str
        )
    } else {
        format!(
            "**{}**\n\nConffile - not found in debhelper staging directory",
            path_str
        )
    };

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;

    fn hover(text: &str, line: u32, col: u32) -> Option<Hover> {
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        get_hover(src, Position::new(line, col), None)
    }

    fn md(h: &Hover) -> &str {
        match &h.contents {
            HoverContents::Markup(m) => &m.value,
            _ => panic!("expected markup"),
        }
    }

    #[test]
    fn test_hover_on_path() {
        let h = hover("/etc/myapp/config.conf\n", 0, 5).unwrap();
        assert!(md(&h).contains("/etc/myapp/config.conf"));
    }

    #[test]
    fn test_hover_on_remove_on_upgrade_flag() {
        let h = hover("remove-on-upgrade /etc/myapp/old.conf\n", 0, 5).unwrap();
        assert!(md(&h).contains("remove-on-upgrade"));
        assert!(md(&h).contains("Remove"));
    }

    #[test]
    fn test_hover_empty_line_returns_none() {
        assert!(hover("\n", 0, 0).is_none());
    }
}
