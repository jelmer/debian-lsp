//! On-type formatting for debian/upstream/metadata files.
//!
//! Handles two triggers:
//! - `\n`: insert indentation matching the sub-field key column when inside a
//!   mapping-list entry (e.g. Registry, Reference, Funding).
//! - `:`:  insert a trailing space after a field-name colon.

use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{Position, Range, TextEdit};
use yaml_edit::{Mapping, YamlFile, YamlNode};

use super::fields::{FieldValueType, UPSTREAM_FIELDS};

/// Generate on-type formatting edits for upstream/metadata files.
pub fn on_type_formatting(
    source_text: &str,
    position: Position,
    ch: &str,
) -> Option<Vec<TextEdit>> {
    match ch {
        "\n" => on_type_newline(source_text, position),
        ":" => on_type_colon(source_text, position),
        _ => None,
    }
}

/// After typing a newline, if the previous line is inside a YAML mapping that
/// belongs to a mapping-list field, insert indentation so the cursor lands at
/// the correct column for the next sub-field key.
fn on_type_newline(source_text: &str, position: Position) -> Option<Vec<TextEdit>> {
    if position.line == 0 {
        return None;
    }

    // Don't add indentation if the current line has non-whitespace content.
    let current_line = source_text
        .lines()
        .nth(position.line as usize)
        .unwrap_or("");
    if current_line.contains(|c: char| !c.is_whitespace()) {
        return None;
    }

    let indent = indent_for_position(source_text, position)?;

    let indent_str = " ".repeat(indent);

    // If the line already has the right indentation, don't emit an edit.
    if current_line == indent_str {
        return None;
    }

    // Replace any existing whitespace on the line with the correct indent.
    let line_start = Position::new(position.line, 0);
    let line_end = Position::new(position.line, current_line.len() as u32);
    Some(vec![TextEdit {
        range: Range {
            start: line_start,
            end: line_end,
        },
        new_text: indent_str,
    }])
}

/// Compute the indentation (as a number of spaces) that should be inserted
/// on a new line at `position`, or `None` if no auto-indent applies.
fn indent_for_position(source_text: &str, position: Position) -> Option<usize> {
    let parse = YamlFile::parse(source_text);
    let yaml_file = parse.tree();
    let doc = yaml_file.document()?;
    let mapping = doc.as_mapping()?;

    let prev_line_idx = (position.line - 1) as usize;
    let prev_line = source_text.lines().nth(prev_line_idx)?;

    // Find which top-level entry the previous line belongs to.
    // Use a byte offset in the middle of the previous line.
    let prev_line_offset = source_text
        .lines()
        .take(prev_line_idx)
        .map(|l| l.len() + 1)
        .sum::<usize>();
    // Use offset at the start of non-whitespace content on the previous line.
    let content_start = prev_line.len() - prev_line.trim_start().len();
    let probe_offset = (prev_line_offset + content_start) as u32;

    // Find the top-level entry containing this offset.
    let entry = mapping.entries().find(|e| {
        let range = e.syntax().text_range();
        let start: u32 = range.start().into();
        let end: u32 = range.end().into();
        probe_offset >= start && probe_offset < end
    })?;

    // Get the field name and check if it's a mapping-list field.
    let field_name = match entry.key_node()? {
        YamlNode::Scalar(s) => s.as_string(),
        _ => return None,
    };
    let lower = field_name.to_ascii_lowercase();
    let field = UPSTREAM_FIELDS
        .iter()
        .find(|f| f.name.to_ascii_lowercase() == lower)?;

    match field.value_type {
        FieldValueType::MappingList(_) => {}
        _ => return None,
    }

    // Compute the default indent from the top-level key position.
    let default_indent = entry
        .key_node()
        .and_then(|key| match key {
            YamlNode::Scalar(s) => Some(s.start_position(source_text).column - 1 + 2),
            _ => None,
        })
        .unwrap_or(2);

    // Find the inner mapping that the previous line belongs to.
    let value_node = match entry.value_node() {
        Some(v) => v,
        // No value yet (e.g. just "Reference:\n") — use default indent.
        None => return Some(default_indent),
    };
    let sequence = match value_node.as_sequence() {
        Some(s) => s,
        None => return Some(default_indent),
    };

    for item in sequence.values() {
        if let YamlNode::Mapping(inner_mapping) = item {
            let indent = mapping_key_column(&inner_mapping, source_text);
            if let Some(indent) = indent {
                let range = inner_mapping.syntax().text_range();
                let start: u32 = range.start().into();
                let end: u32 = range.end().into();
                if probe_offset >= start && probe_offset < end {
                    return Some(indent);
                }
            }
        }
    }

    // Previous line is in the sequence but not in any mapping — e.g. on the
    // `- ` line itself. The indent should match the first mapping's key column,
    // or derive from the entry's key column + 2.
    let from_seq: Vec<_> = sequence.values().collect();
    let indent = from_seq
        .iter()
        .find_map(|item| {
            item.as_mapping()
                .and_then(|m| mapping_key_column(m, source_text))
        })
        .unwrap_or(default_indent);
    Some(indent)
}

/// Get the 0-indexed column of the first key in a mapping.
fn mapping_key_column(mapping: &Mapping, source_text: &str) -> Option<usize> {
    mapping
        .entries()
        .next()
        .and_then(|e| e.key_node())
        .and_then(|key| match key {
            YamlNode::Scalar(s) => Some(s.start_position(source_text).column - 1),
            _ => None,
        })
}

/// After typing `:`, insert a space if this looks like a field separator.
fn on_type_colon(source_text: &str, position: Position) -> Option<Vec<TextEdit>> {
    // Check that the character after the cursor isn't already a space.
    let line = source_text.lines().nth(position.line as usize)?;
    let col = position.character as usize;
    if line[col..].starts_with(' ') {
        return None;
    }

    // Check that the text before the colon on this line looks like a field name
    // (i.e. no colon before this one on the same line, which would mean we're
    // in a value like a URL).
    let before_colon = &line[..col.saturating_sub(1)];
    let trimmed = before_colon.trim_start_matches("- ").trim_start();
    if trimmed.contains(':') {
        return None;
    }

    Some(vec![TextEdit {
        range: Range {
            start: position,
            end: position,
        },
        new_text: " ".to_string(),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_newline_after_subfield_inserts_indent() {
        let text = "Reference:\n  - Type: Book\n\n";
        let edits = on_type_formatting(text, Position::new(2, 0), "\n").unwrap();
        assert_eq!(edits.len(), 1);
        // "Type" starts at column 4 (after "  - "), so indent should be 4 spaces.
        assert_eq!(edits[0].new_text, "    ");
    }

    #[test]
    fn test_newline_after_subfield_no_dash_prefix() {
        // "- Type: Book" with dash at column 0 → Type at column 2.
        let text = "Reference:\n- Type: Book\n\n";
        let edits = on_type_formatting(text, Position::new(2, 0), "\n").unwrap();
        assert_eq!(edits[0].new_text, "  ");
    }

    #[test]
    fn test_newline_after_continuation_subfield() {
        let text = "Registry:\n  - Name: PyPI\n    Entry: example\n\n";
        let edits = on_type_formatting(text, Position::new(3, 0), "\n").unwrap();
        assert_eq!(edits[0].new_text, "    ");
    }

    #[test]
    fn test_newline_after_scalar_field_no_indent() {
        let text = "Repository: https://example.com\n\n";
        let result = on_type_formatting(text, Position::new(1, 0), "\n");
        assert!(result.is_none());
    }

    #[test]
    fn test_newline_at_start_of_file() {
        let text = "\n";
        let result = on_type_formatting(text, Position::new(0, 0), "\n");
        assert!(result.is_none());
    }

    #[test]
    fn test_newline_already_has_content() {
        let text = "Reference:\n  - Type: Book\nfoo\n";
        let result = on_type_formatting(text, Position::new(2, 0), "\n");
        assert!(result.is_none());
    }

    #[test]
    fn test_newline_editor_auto_indented_wrong() {
        // Editor auto-indented with 2 spaces, but correct indent is 4.
        let text = "Reference:\n  - Type: Book\n  \n";
        let edits = on_type_formatting(text, Position::new(2, 2), "\n").unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "    ");
        // Should replace the existing 2 spaces.
        assert_eq!(edits[0].range.start, Position::new(2, 0));
        assert_eq!(edits[0].range.end, Position::new(2, 2));
    }

    #[test]
    fn test_newline_editor_auto_indented_correct() {
        // Editor already indented correctly — no edit needed.
        let text = "Reference:\n  - Type: Book\n    \n";
        let result = on_type_formatting(text, Position::new(2, 4), "\n");
        assert!(result.is_none());
    }

    #[test]
    fn test_newline_trailing_newline_no_empty_line_in_lines() {
        // This simulates what the editor sends: text ends with \n, and position
        // is on the line after the last \n. Rust's .lines() won't yield that
        // empty trailing line.
        let text = "Reference:\n  - Type: Book\n";
        let edits = on_type_formatting(text, Position::new(2, 0), "\n").unwrap();
        assert_eq!(edits[0].new_text, "    ");
    }

    #[test]
    fn test_newline_after_top_level_mapping_list_key() {
        // User typed "Reference:" and pressed Enter. Should indent for a
        // sequence item.
        let text = "Reference:\n";
        let edits = on_type_formatting(text, Position::new(1, 0), "\n").unwrap();
        // Default indent is key_column(0) + 2 = 2 spaces.
        assert_eq!(edits[0].new_text, "  ");
    }

    #[test]
    fn test_colon_inserts_space() {
        let text = "Repository:\n";
        let edits = on_type_formatting(text, Position::new(0, 11), ":").unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, " ");
    }

    #[test]
    fn test_colon_with_existing_space_no_edit() {
        let text = "Repository: foo\n";
        let result = on_type_formatting(text, Position::new(0, 11), ":");
        assert!(result.is_none());
    }

    #[test]
    fn test_colon_in_url_no_edit() {
        let text = "Repository: https:\n";
        // Colon after "https" — there's already a colon earlier on the line.
        let result = on_type_formatting(text, Position::new(0, 18), ":");
        assert!(result.is_none());
    }

    #[test]
    fn test_colon_after_subfield_name() {
        let text = "Registry:\n  - Name:\n";
        let edits = on_type_formatting(text, Position::new(1, 9), ":").unwrap();
        assert_eq!(edits[0].new_text, " ");
    }
}
