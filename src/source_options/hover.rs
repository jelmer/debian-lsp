//! Hover documentation for debian/source/options files.

use tower_lsp_server::ls_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};

use super::fields::find_option;

/// Return the option name at `col` on `line`, if the cursor is on the option
/// name (i.e. before any `=`). Comment and blank lines yield nothing.
fn option_at_position(line: &str, col: usize) -> Option<&str> {
    let content = line.trim_start();
    if content.is_empty() || content.starts_with('#') {
        return None;
    }

    let name_end = line.find('=').unwrap_or(line.len());
    if col > name_end {
        return None;
    }

    let name = line[..name_end].trim();
    if name.is_empty() {
        return None;
    }
    Some(name)
}

/// Get hover documentation for a source options file at the given position.
pub fn get_hover(text: &str, position: Position) -> Option<Hover> {
    let line = text.lines().nth(position.line as usize).unwrap_or("");
    let name = option_at_position(line, position.character as usize)?;
    let opt = find_option(name)?;

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("**`{}`**\n\n{}", opt.name, opt.description),
        }),
        range: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hover_value(text: &str, line: u32, character: u32) -> Option<String> {
        let h = get_hover(text, Position { line, character })?;
        match h.contents {
            HoverContents::Markup(m) => Some(m.value),
            _ => None,
        }
    }

    #[test]
    fn test_hover_boolean_option() {
        let v = hover_value("single-debian-patch\n", 0, 3).unwrap();
        assert_eq!(
            v,
            "**`single-debian-patch`**\n\nUse debian/patches/debian-changes as automatic patch"
        );
    }

    #[test]
    fn test_hover_option_with_value() {
        let v = hover_value("compression = xz\n", 0, 2).unwrap();
        assert_eq!(
            v,
            "**`compression`**\n\nSelect compression to use (supported: bzip2, gzip, lzma, xz)"
        );
    }

    #[test]
    fn test_no_hover_on_value() {
        assert_eq!(hover_value("compression = xz\n", 0, 15), None);
    }

    #[test]
    fn test_no_hover_on_comment() {
        assert_eq!(hover_value("# compression\n", 0, 4), None);
    }

    #[test]
    fn test_no_hover_on_unknown_option() {
        assert_eq!(hover_value("not-a-real-option\n", 0, 3), None);
    }

    #[test]
    fn test_no_hover_on_blank_line() {
        assert_eq!(hover_value("\n", 0, 0), None);
    }
}
