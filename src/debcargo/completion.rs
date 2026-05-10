use toml_edit::Document;
use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, InsertTextFormat, Position};

use super::fields::{MULTI_ARCH_VALUES, PACKAGE_KEYS, SOURCE_KEYS, TOP_LEVEL_KEYS};

/// Context for where the cursor is in a debcargo.toml file.
enum CursorContext {
    /// Cursor is at a key position in the top-level table.
    TopLevelKey,
    /// Cursor is at a key position in the [source] table.
    SourceKey,
    /// Cursor is at a key position in a [packages.KEY] table.
    PackageKey,
    /// Cursor is on the value side of the `multi_arch` key.
    MultiArchValue,
    /// No completions available at this position.
    None,
}

#[derive(PartialEq)]
enum TableKind {
    TopLevel,
    Source,
    Package,
    Unknown,
}

/// Determine which TOML table the cursor line belongs to by scanning
/// backwards for a `[…]` header line.
fn find_current_table(lines: &[&str], line_idx: usize) -> TableKind {
    if lines.is_empty() {
        return TableKind::TopLevel;
    }
    let bound = line_idx.min(lines.len() - 1);
    for i in (0..=bound).rev() {
        let line = lines[i].trim();
        if line.starts_with("[packages.") {
            return TableKind::Package;
        }
        if line == "[source]" {
            return TableKind::Source;
        }
        if line.starts_with('[') && !line.starts_with("[[") {
            return TableKind::Unknown;
        }
    }
    TableKind::TopLevel
}

/// Determine the cursor context from the document text and position.
fn determine_context(text: &str, position: Position) -> CursorContext {
    let line_idx = position.line as usize;
    let col = position.character as usize;
    let lines: Vec<&str> = text.lines().collect();
    let current_line = lines.get(line_idx).copied().unwrap_or("");

    // If cursor is after '=' on this line, we are in a value position.
    if let Some(eq_pos) = current_line.find('=') {
        if col > eq_pos {
            let key = current_line[..eq_pos].trim();
            let table = find_current_table(&lines, line_idx);
            if table == TableKind::Package && key == "multi_arch" {
                return CursorContext::MultiArchValue;
            }
            return CursorContext::None;
        }
    }

    // Cursor is at a key position.
    match find_current_table(&lines, line_idx) {
        TableKind::TopLevel => CursorContext::TopLevelKey,
        TableKind::Source => CursorContext::SourceKey,
        TableKind::Package => CursorContext::PackageKey,
        TableKind::Unknown => CursorContext::None,
    }
}

/// Parse the document with toml_edit for structural information (e.g.
/// filtering out keys already present). Returns `None` for invalid TOML.
fn try_parse(text: &str) -> Option<Document> {
    text.parse::<Document>().ok()
}

/// Get completions for a debcargo.toml file at the given position.
pub fn get_completions(text: &str, position: Position) -> Vec<CompletionItem> {
    let doc = try_parse(text);

    match determine_context(text, position) {
        CursorContext::TopLevelKey => {
            let existing: Vec<String> = doc
                .as_ref()
                .map(|d| d.iter().map(|(k, _)| k.to_string()).collect())
                .unwrap_or_default();
            TOP_LEVEL_KEYS
                .iter()
                .filter(|k| !existing.contains(&k.name.to_string()))
                .map(|k| CompletionItem {
                    label: k.name.to_string(),
                    kind: Some(CompletionItemKind::PROPERTY),
                    detail: Some(k.description.to_string()),
                    insert_text: Some(format!("{} = ", k.name)),
                    ..Default::default()
                })
                .collect()
        }
        CursorContext::SourceKey => {
            let existing: Vec<String> = doc
                .as_ref()
                .and_then(|d| d.get("source"))
                .and_then(|v| v.as_table())
                .map(|t| t.iter().map(|(k, _)| k.to_string()).collect())
                .unwrap_or_default();
            SOURCE_KEYS
                .iter()
                .filter(|k| !existing.contains(&k.name.to_string()))
                .map(|k| CompletionItem {
                    label: k.name.to_string(),
                    kind: Some(CompletionItemKind::PROPERTY),
                    detail: Some(k.description.to_string()),
                    insert_text: Some(format!("{} = ", k.name)),
                    ..Default::default()
                })
                .collect()
        }
        CursorContext::PackageKey => PACKAGE_KEYS
            .iter()
            .map(|k| CompletionItem {
                label: k.name.to_string(),
                kind: Some(CompletionItemKind::PROPERTY),
                detail: Some(k.description.to_string()),
                insert_text: Some(format!("{} = ", k.name)),
                ..Default::default()
            })
            .collect(),
        CursorContext::MultiArchValue => MULTI_ARCH_VALUES
            .iter()
            .map(|(value, description)| CompletionItem {
                label: format!("\"{value}\""),
                kind: Some(CompletionItemKind::VALUE),
                detail: Some((*description).to_string()),
                insert_text: Some(format!("\"{value}\"")),
                insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                ..Default::default()
            })
            .collect(),
        CursorContext::None => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(text: &str, position: Position) -> Vec<String> {
        get_completions(text, position)
            .into_iter()
            .map(|c| c.label)
            .collect()
    }

    #[test]
    fn test_top_level_key_completions() {
        let items = labels("", Position::new(0, 0));
        let expected: Vec<String> = TOP_LEVEL_KEYS.iter().map(|k| k.name.to_string()).collect();
        assert_eq!(items, expected);
    }

    #[test]
    fn test_source_key_completions() {
        let text = "[source]\n";
        let items = labels(text, Position::new(1, 0));
        let expected: Vec<String> = SOURCE_KEYS.iter().map(|k| k.name.to_string()).collect();
        assert_eq!(items, expected);
    }

    #[test]
    fn test_package_key_completions() {
        let text = "[packages.lib]\n";
        let items = labels(text, Position::new(1, 0));
        let expected: Vec<String> = PACKAGE_KEYS.iter().map(|k| k.name.to_string()).collect();
        assert_eq!(items, expected);
    }

    #[test]
    fn test_multi_arch_value_completions() {
        let text = "[packages.bin]\nmulti_arch = ";
        let items = labels(text, Position::new(1, 13));
        assert_eq!(
            items,
            vec!["\"no\"", "\"same\"", "\"foreign\"", "\"allowed\""]
        );
    }

    #[test]
    fn test_no_completions_in_other_value() {
        let text = "[packages.lib]\nbreaks = ";
        let items = labels(text, Position::new(1, 9));
        assert_eq!(items, Vec::<String>::new());
    }

    #[test]
    fn test_existing_top_level_key_excluded() {
        let text = "overlay = \".\"\n";
        let items = labels(text, Position::new(1, 0));
        let expected: Vec<String> = TOP_LEVEL_KEYS
            .iter()
            .filter(|k| k.name != "overlay")
            .map(|k| k.name.to_string())
            .collect();
        assert_eq!(items, expected);
    }

    #[test]
    fn test_completions_have_insert_text_with_equals() {
        let items = get_completions("", Position::new(0, 0));
        let insert_texts: Vec<&str> = items
            .iter()
            .map(|c| c.insert_text.as_deref().unwrap_or(""))
            .collect();
        let expected: Vec<String> = TOP_LEVEL_KEYS
            .iter()
            .map(|k| format!("{} = ", k.name))
            .collect();
        assert_eq!(
            insert_texts,
            expected.iter().map(|s| s.as_str()).collect::<Vec<_>>()
        );
    }
}
