use tower_lsp_server::ls_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};

use super::fields::{PACKAGE_KEYS, SOURCE_KEYS, TOP_LEVEL_KEYS};

/// Find the key name on the current line at the given position, if the cursor
/// is on a key (i.e., before any `=` on the line).
fn key_at_position(line: &str, col: usize) -> Option<&str> {
    let eq_pos = line.find('=')?;
    if col >= eq_pos {
        return None;
    }
    let key = line[..eq_pos].trim();
    if key.is_empty() || key.starts_with('#') {
        return None;
    }
    Some(key)
}

#[derive(Debug, PartialEq)]
enum TableContext {
    TopLevel,
    Source,
    Package,
    Unknown,
}

fn find_current_table(lines: &[&str], line_idx: usize) -> TableContext {
    if lines.is_empty() {
        return TableContext::TopLevel;
    }
    let bound = line_idx.min(lines.len() - 1);
    for i in (0..=bound).rev() {
        let line = lines[i].trim();
        if line.starts_with("[packages.") {
            return TableContext::Package;
        }
        if line == "[source]" {
            return TableContext::Source;
        }
        if line.starts_with('[') && !line.starts_with("[[") {
            return TableContext::Unknown;
        }
    }
    TableContext::TopLevel
}

/// Get hover documentation for a debcargo.toml file at the given position.
pub fn get_hover(text: &str, position: Position) -> Option<Hover> {
    let line_idx = position.line as usize;
    let col = position.character as usize;
    let lines: Vec<&str> = text.lines().collect();
    let current_line = lines.get(line_idx).copied().unwrap_or("");

    let key = key_at_position(current_line, col)?;

    let description = match find_current_table(&lines, line_idx) {
        TableContext::TopLevel => TOP_LEVEL_KEYS
            .iter()
            .find(|k| k.name == key)
            .map(|k| k.description),
        TableContext::Source => SOURCE_KEYS
            .iter()
            .find(|k| k.name == key)
            .map(|k| k.description),
        TableContext::Package => PACKAGE_KEYS
            .iter()
            .find(|k| k.name == key)
            .map(|k| k.description),
        TableContext::Unknown => None,
    }?;

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("**`{key}`**\n\n{description}"),
        }),
        range: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hover_value(text: &str, position: Position) -> Option<String> {
        let h = get_hover(text, position)?;
        match h.contents {
            HoverContents::Markup(m) => Some(m.value),
            _ => None,
        }
    }

    #[test]
    fn test_hover_top_level_key() {
        let text = "uploaders = []\n";
        assert_eq!(
            hover_value(text, Position::new(0, 3)).as_deref(),
            Some("**`uploaders`**\n\nUploaders for the package (affects Uploaders: field and debian/copyright)")
        );
    }

    #[test]
    fn test_hover_source_key() {
        let text = "[source]\nsection = \"rust\"\n";
        assert_eq!(
            hover_value(text, Position::new(1, 2)).as_deref(),
            Some("**`section`**\n\nSection override for the source package")
        );
    }

    #[test]
    fn test_hover_package_key() {
        let text = "[packages.lib]\nbreaks = []\n";
        assert_eq!(
            hover_value(text, Position::new(1, 2)).as_deref(),
            Some("**`breaks`**\n\nBreaks relationships for the package")
        );
    }

    #[test]
    fn test_hover_on_value_returns_none() {
        let text = "overlay = \".\"\n";
        assert_eq!(get_hover(text, Position::new(0, 11)), None);
    }

    #[test]
    fn test_hover_unknown_key_returns_none() {
        let text = "unknown_key = true\n";
        assert_eq!(get_hover(text, Position::new(0, 3)), None);
    }
}
