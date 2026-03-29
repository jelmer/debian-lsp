use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};
use yaml_edit::{Document, Mapping, YamlFile, YamlNode};

use super::fields::{
    get_subfield_name_completions, get_subfield_value_completions, FieldValueType, UPSTREAM_FIELDS,
};
use crate::position::try_position_to_offset;

/// Cursor context within the upstream/metadata YAML document.
enum CursorContext {
    /// Cursor is at a position where a new top-level field name can be typed.
    TopLevelKey,
    /// Cursor is on a top-level scalar value (no special completions).
    TopLevelValue,
    /// Cursor is inside a mapping-list value, on a sub-field key position.
    SubFieldKey {
        subfields: &'static [super::fields::SubField],
        prefix: String,
        /// Column (0-indexed) where sub-field keys should start.
        indent: u32,
    },
    /// Cursor is inside a mapping-list value, on a sub-field value position.
    SubFieldValue {
        subfields: &'static [super::fields::SubField],
        subfield_name: String,
        prefix: String,
        /// Column (0-indexed) where sub-field keys should start (for newline after value).
        indent: u32,
    },
    /// No completions available (e.g. inside a scalar-list value).
    None,
}

/// Check whether a byte offset falls strictly within the range of a syntax node.
/// Uses exclusive end to avoid matching trailing whitespace/newlines.
fn offset_in_node_exclusive(node: &impl AstNode<Language = yaml_edit::Lang>, offset: u32) -> bool {
    let range = node.syntax().text_range();
    let start: u32 = range.start().into();
    let end: u32 = range.end().into();
    offset >= start && offset < end
}

/// Find the top-level mapping entry whose range contains the cursor offset.
fn find_entry_at_offset(mapping: &Mapping, offset: u32) -> Option<yaml_edit::MappingEntry> {
    mapping
        .entries()
        .find(|entry| offset_in_node_exclusive(entry, offset))
}

/// Compute the column (0-indexed) at which sub-field keys start in a mapping.
fn subfield_indent(mapping: &Mapping, source_text: &str) -> Option<u32> {
    mapping
        .entries()
        .next()
        .and_then(|e| e.key_node())
        .and_then(|key| match key {
            YamlNode::Scalar(s) => {
                // yaml_edit LineColumn is 1-indexed
                Some(s.start_position(source_text).column as u32 - 1)
            }
            _ => None,
        })
}

/// Compute the default indent for a sequence item's sub-fields based on the
/// top-level key that owns the sequence. The convention is key_column + 2.
fn default_subfield_indent(entry: &yaml_edit::MappingEntry, source_text: &str) -> u32 {
    entry
        .key_node()
        .and_then(|key| match key {
            YamlNode::Scalar(s) => Some(s.start_position(source_text).column as u32 - 1 + 2),
            _ => None,
        })
        .unwrap_or(2)
}

/// Get the text of a scalar key node.
fn key_text(entry: &yaml_edit::MappingEntry) -> Option<String> {
    match entry.key_node()? {
        YamlNode::Scalar(s) => Some(s.as_string()),
        _ => Option::None,
    }
}

/// Look up the field definition for a top-level key name.
fn lookup_field(key: &str) -> Option<&'static super::fields::UpstreamField> {
    let lower = key.to_ascii_lowercase();
    UPSTREAM_FIELDS
        .iter()
        .find(|f| f.name.to_ascii_lowercase() == lower)
}

/// Determine the cursor context by walking the YAML CST.
fn determine_context(doc: &Document, source_text: &str, offset: u32) -> CursorContext {
    let mapping = match doc.as_mapping() {
        Some(m) => m,
        // Document isn't a mapping (or is empty) — offer top-level field names.
        None => return CursorContext::TopLevelKey,
    };

    // Check if cursor falls within an existing top-level entry (exclusive end).
    // If not found, check if the cursor is right at the boundary of the last
    // entry. This handles cases like "Reference:\n" where the entry range is
    // 0..11 and offset is 11.
    let entry = find_entry_at_offset(&mapping, offset).or_else(|| {
        mapping
            .entries()
            .filter(|e| {
                let range = e.syntax().text_range();
                let end: u32 = range.end().into();
                offset == end
            })
            .last()
    });

    let entry = match entry {
        Some(e) => e,
        None => return CursorContext::TopLevelKey,
    };

    // Check if cursor is on the key portion of the entry.
    if let Some(YamlNode::Scalar(key_scalar)) = entry.key_node() {
        let range = key_scalar.byte_range();
        if offset >= range.start && offset < range.end {
            return CursorContext::TopLevelKey;
        }
    }

    // Look up the field to determine its value type.
    let field_name = match key_text(&entry) {
        Some(name) => name,
        None => return CursorContext::None,
    };

    let field = match lookup_field(&field_name) {
        Some(f) => f,
        None => return CursorContext::None,
    };

    // For scalar fields, if the cursor is on a different line than the key,
    // the user is likely starting a new top-level field.
    let cursor_line = yaml_edit::byte_offset_to_line_column(source_text, offset as usize).line;
    let key_line = entry
        .key_node()
        .map(|key| match key {
            YamlNode::Scalar(s) => {
                yaml_edit::byte_offset_to_line_column(source_text, s.byte_range().start as usize)
                    .line
            }
            _ => 0,
        })
        .unwrap_or(0);

    // Check if cursor is inside the value node's range.
    let in_value = entry.value_node().is_some_and(|v| match &v {
        YamlNode::Scalar(s) => {
            let r = s.byte_range();
            offset >= r.start && offset < r.end
        }
        YamlNode::Sequence(s) => offset_in_node_exclusive(s, offset),
        YamlNode::Mapping(m) => offset_in_node_exclusive(m, offset),
        _ => false,
    });

    match field.value_type {
        FieldValueType::Scalar => {
            if cursor_line == key_line || in_value {
                CursorContext::TopLevelValue
            } else {
                CursorContext::TopLevelKey
            }
        }
        FieldValueType::ScalarList => {
            if cursor_line == key_line || in_value {
                CursorContext::None
            } else {
                CursorContext::TopLevelKey
            }
        }
        FieldValueType::MappingList(subfields) => {
            determine_mapping_list_context(entry, source_text, offset, subfields)
        }
    }
}

/// Determine context within a mapping-list value (e.g. Registry, Reference).
///
/// The value is a sequence of mappings. We need to find which mapping the
/// cursor is in, and whether it's on a sub-field key or value.
fn determine_mapping_list_context(
    entry: yaml_edit::MappingEntry,
    source_text: &str,
    offset: u32,
    subfields: &'static [super::fields::SubField],
) -> CursorContext {
    let default_indent = default_subfield_indent(&entry, source_text);

    // Don't offer sub-field completions on the same line as the top-level key.
    // E.g. "Reference:|" should not complete "Author:" right after the colon.
    let key_line = entry
        .key_node()
        .map(|key| match key {
            YamlNode::Scalar(s) => {
                yaml_edit::byte_offset_to_line_column(source_text, s.byte_range().start as usize)
                    .line
            }
            _ => 0,
        })
        .unwrap_or(0);
    let cursor_line = yaml_edit::byte_offset_to_line_column(source_text, offset as usize).line;
    if cursor_line == key_line {
        return CursorContext::None;
    }

    let value_node = match entry.value_node() {
        Some(v) => v,
        // Value not yet typed — offer sub-field names.
        None => {
            return CursorContext::SubFieldKey {
                subfields,
                prefix: String::new(),
                indent: default_indent,
            }
        }
    };

    let sequence = match value_node.as_sequence() {
        Some(s) => s,
        // Value exists but isn't a sequence — might be partially typed.
        None => {
            // Could be a mapping directly (single item without sequence notation).
            if let Some(inner_mapping) = value_node.as_mapping() {
                return determine_inner_mapping_context(
                    inner_mapping,
                    source_text,
                    offset,
                    subfields,
                    default_indent,
                );
            }
            return CursorContext::SubFieldKey {
                subfields,
                prefix: String::new(),
                indent: default_indent,
            };
        }
    };

    // Walk sequence items to find which one contains the cursor.
    for item in sequence.values() {
        if let YamlNode::Mapping(inner_mapping) = item {
            if offset_in_node_exclusive(&inner_mapping, offset) {
                return determine_inner_mapping_context(
                    &inner_mapping,
                    source_text,
                    offset,
                    subfields,
                    default_indent,
                );
            }
        }
    }

    // Cursor is in the sequence but not inside any mapping item —
    // likely on a new `- ` line, offer sub-field names.
    // Derive indent from the last sequence item's mapping if available.
    let indent = sequence
        .values()
        .filter_map(|item| item.as_mapping().cloned())
        .last()
        .and_then(|m| subfield_indent(&m, source_text))
        .unwrap_or(default_indent);
    CursorContext::SubFieldKey {
        subfields,
        prefix: String::new(),
        indent,
    }
}

/// Determine context within an inner mapping (a single item in a mapping list).
fn determine_inner_mapping_context(
    mapping: &Mapping,
    source_text: &str,
    offset: u32,
    subfields: &'static [super::fields::SubField],
    default_indent: u32,
) -> CursorContext {
    let indent = subfield_indent(mapping, source_text).unwrap_or(default_indent);

    // Check each sub-entry. We need to find entries where the cursor is either
    // within the entry range, or just past the entry on the same line (for
    // values that are empty or where the cursor is at the end of the value).
    for sub_entry in mapping.entries() {
        if let Some(YamlNode::Scalar(key_scalar)) = sub_entry.key_node() {
            let key_range = key_scalar.byte_range();

            if offset < key_range.end && offset >= key_range.start {
                // Cursor is on the sub-field key.
                let prefix = &source_text[key_range.start as usize..offset as usize];
                return CursorContext::SubFieldKey {
                    subfields,
                    prefix: prefix.to_string(),
                    indent,
                };
            }

            // Check if cursor is past the key (in the value area).
            // The value area starts at or after the colon (key_range.end).
            // We consider the cursor to be on this entry's value if
            // offset >= key_end and they are on the same line.
            if offset >= key_range.end {
                let key_end_line =
                    yaml_edit::byte_offset_to_line_column(source_text, key_range.end as usize).line;
                let cursor_line =
                    yaml_edit::byte_offset_to_line_column(source_text, offset as usize).line;

                if cursor_line == key_end_line {
                    let subfield_name = key_scalar.as_string();
                    let prefix = match sub_entry.value_node() {
                        Some(YamlNode::Scalar(val_scalar)) => {
                            let val_range = val_scalar.byte_range();
                            if offset >= val_range.start {
                                source_text
                                    .get(val_range.start as usize..offset as usize)
                                    .unwrap_or("")
                                    .to_string()
                            } else {
                                String::new()
                            }
                        }
                        _ => String::new(),
                    };

                    return CursorContext::SubFieldValue {
                        subfields,
                        subfield_name,
                        prefix,
                        indent,
                    };
                }
            }
        }
    }

    // Cursor is in the mapping but not inside any entry — new sub-field.
    CursorContext::SubFieldKey {
        subfields,
        prefix: String::new(),
        indent,
    }
}

/// Get completions for a debian/upstream/metadata file at the given position.
pub fn get_completions(source_text: &str, position: Position) -> Vec<CompletionItem> {
    let offset = match try_position_to_offset(source_text, position) {
        Some(o) => u32::from(o),
        None => return get_field_completions(),
    };

    let parse = YamlFile::parse(source_text);
    let yaml_file = parse.tree();
    let doc = match yaml_file.document() {
        Some(d) => d,
        None => return get_field_completions(),
    };

    match determine_context(&doc, source_text, offset) {
        CursorContext::TopLevelKey => get_field_completions(),
        CursorContext::TopLevelValue => vec![],
        CursorContext::SubFieldKey {
            subfields,
            ref prefix,
            indent,
        } => get_subfield_name_completions(subfields, prefix, indent),
        CursorContext::SubFieldValue {
            subfields,
            ref subfield_name,
            ref prefix,
            indent,
        } => get_subfield_value_completions(subfields, subfield_name, prefix, indent),
        CursorContext::None => vec![],
    }
}

/// Generate field name completions for all known DEP-12 fields.
fn get_field_completions() -> Vec<CompletionItem> {
    UPSTREAM_FIELDS
        .iter()
        .map(|field| CompletionItem {
            label: field.name.to_string(),
            kind: Some(CompletionItemKind::FIELD),
            detail: Some(field.description.to_string()),
            insert_text: Some(format!("{}: ", field.name)),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp_server::ls_types::InsertTextFormat;

    #[test]
    fn test_get_completions_at_start_of_empty_line() {
        let text = "Repository: https://example.com\n\n";
        let completions = get_completions(text, Position::new(1, 0));

        assert_eq!(completions.len(), UPSTREAM_FIELDS.len());
        assert_eq!(completions[0].label, "Repository");
        assert_eq!(
            completions[0].detail.as_deref(),
            Some("URL of the upstream source repository")
        );
    }

    #[test]
    fn test_get_completions_on_value() {
        let text = "Repository: https://example.com\n";
        let completions = get_completions(text, Position::new(0, 12));
        assert_eq!(completions.len(), 0);
    }

    #[test]
    fn test_get_completions_on_existing_field_key() {
        let text = "Repository: https://example.com\n";
        let completions = get_completions(text, Position::new(0, 0));
        // Cursor is on the key of an existing entry.
        assert_eq!(completions.len(), UPSTREAM_FIELDS.len());
    }

    #[test]
    fn test_get_completions_typing_field_name() {
        let text = "Repository: https://example.com\nBug";
        let completions = get_completions(text, Position::new(1, 3));
        assert_eq!(completions.len(), UPSTREAM_FIELDS.len());
    }

    #[test]
    fn test_get_completions_partial_field_name_at_col_zero() {
        let text = "Repository: https://example.com\nBug";
        let completions = get_completions(text, Position::new(1, 0));
        assert_eq!(completions.len(), UPSTREAM_FIELDS.len());
    }

    #[test]
    fn test_get_completions_on_indented_scalar_list() {
        // Other-References is a ScalarList — no sub-field completions.
        let text = "Other-References:\n  - https://example.com\n";
        let completions = get_completions(text, Position::new(1, 4));
        assert_eq!(completions.len(), 0);
    }

    #[test]
    fn test_get_completions_indented_line() {
        // Indented lines are continuation/value lines — no field completions
        let text = "Repository: https://example.com\n  indented\n";
        let completions = get_completions(text, Position::new(1, 2));
        assert_eq!(completions.len(), 0);
    }

    #[test]
    fn test_get_completions_empty_file() {
        let completions = get_completions("", Position::new(0, 0));
        assert_eq!(completions.len(), UPSTREAM_FIELDS.len());
    }

    #[test]
    fn test_get_completions_line_beyond_file() {
        let text = "Repository: https://example.com\n";
        let completions = get_completions(text, Position::new(5, 0));
        // Line 5 doesn't exist — falls back to field completions.
        assert_eq!(completions.len(), UPSTREAM_FIELDS.len());
    }

    #[test]
    fn test_field_completions_have_insert_text() {
        let completions = get_field_completions();
        assert_eq!(completions.len(), UPSTREAM_FIELDS.len());
        for c in &completions {
            assert_eq!(c.kind, Some(CompletionItemKind::FIELD));
            assert!(c.insert_text.as_ref().unwrap().ends_with(": "));
            assert!(c.detail.is_some());
        }
    }

    #[test]
    fn test_registry_subfield_name_completions() {
        let text = "Registry:\n  - Name: PyPI\n    Entry: example\n";
        let completions = get_completions(text, Position::new(1, 4));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["Name", "Entry"]);
    }

    #[test]
    fn test_registry_name_value_completions() {
        let text = "Registry:\n  - Name: \n";
        let completions = get_completions(text, Position::new(1, 10));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "ASCL",
                "BitBucket",
                "CPAN",
                "Codeberg",
                "SourceForge",
                "GitHub",
                "GitLab",
                "Go",
                "Hackage",
                "Heptapod",
                "Launchpad",
                "Maven",
                "PyPI",
                "Savannah",
                "SourceHut",
                "crates.io",
                "npm",
            ]
        );
    }

    #[test]
    fn test_registry_name_value_completions_with_prefix() {
        let text = "Registry:\n  - Name: Py\n";
        let completions = get_completions(text, Position::new(1, 12));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["PyPI"]);
    }

    #[test]
    fn test_registry_entry_no_value_completions() {
        let text = "Registry:\n  - Name: PyPI\n    Entry: example\n";
        let completions = get_completions(text, Position::new(2, 11));
        assert_eq!(completions.len(), 0);
    }

    #[test]
    fn test_registry_second_subfield_key() {
        let text = "Registry:\n  - Name: PyPI\n    Entry: example\n";
        let completions = get_completions(text, Position::new(2, 4));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["Name", "Entry"]);
    }

    #[test]
    fn test_reference_subfield_name_completions() {
        let text = "Reference:\n  - Type: Article\n    Title: A paper\n";
        let completions = get_completions(text, Position::new(1, 4));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Type", "Title", "Author", "Year", "DOI", "URL", "Journal", "Volume", "EPRINT",
                "ISSN", "Comment",
            ]
        );
    }

    #[test]
    fn test_reference_type_value_completions() {
        let text = "Reference:\n  - Type: \n";
        let completions = get_completions(text, Position::new(1, 10));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Article",
                "Book",
                "Conference",
                "InProceedings",
                "Manual",
                "PhdThesis",
                "TechReport",
                "Unpublished",
            ]
        );
    }

    #[test]
    fn test_reference_type_value_with_prefix() {
        let text = "Reference:\n  - Type: Art\n";
        let completions = get_completions(text, Position::new(1, 13));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["Article"]);
    }

    #[test]
    fn test_funding_subfield_completions() {
        let text = "Funding:\n  - Type: grant\n    Funder: NSF\n";
        let completions = get_completions(text, Position::new(1, 4));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["Type", "Funder", "Grant", "URL"]);
    }

    #[test]
    fn test_non_mapping_list_indented() {
        let text = "Screenshots:\n  - https://example.com\n";
        let completions = get_completions(text, Position::new(1, 4));
        assert_eq!(completions.len(), 0);
    }

    #[test]
    fn test_registry_name_value_right_after_colon() {
        let text = "Registry:\n  - Name:\n";
        let completions = get_completions(text, Position::new(1, 8));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels.len(), 17); // all known registries
        assert_eq!(labels[0], "ASCL");
    }

    #[test]
    fn test_registry_name_value_after_colon_space() {
        let text = "Registry:\n  - Name: \n";
        let completions = get_completions(text, Position::new(1, 10));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels.len(), 17);
        assert_eq!(labels[0], "ASCL");
    }

    #[test]
    fn test_top_level_value_right_after_colon() {
        let text = "Repository:\n";
        let completions = get_completions(text, Position::new(0, 10));
        assert_eq!(completions.len(), 0);
    }

    #[test]
    fn test_mapping_list_no_completions_on_key_line() {
        let text = "Reference:\n";
        let completions = get_completions(text, Position::new(0, 10));
        assert_eq!(completions.len(), 0);
    }

    #[test]
    fn test_mapping_list_no_completions_on_key_line_no_newline() {
        let text = "Reference:";
        let completions = get_completions(text, Position::new(0, 10));
        assert_eq!(completions.len(), 0);
    }

    #[test]
    fn test_unknown_parent_field_indented() {
        let text = "X-Custom:\n  - foo\n";
        let completions = get_completions(text, Position::new(1, 4));
        assert_eq!(completions.len(), 0);
    }

    #[test]
    fn test_subfield_name_after_first_entry_with_indent() {
        let text = "Reference:\n  - Type: Book\n    \n";
        let completions = get_completions(text, Position::new(2, 4));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels.len(), 11); // all Reference sub-fields
        assert_eq!(labels[0], "Type");
        assert_eq!(labels[1], "Title");
    }

    #[test]
    fn test_subfield_name_after_first_entry_less_indent() {
        let text = "Reference:\n  - Type: Book\n   \n";
        let completions = get_completions(text, Position::new(2, 3));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels.len(), 11);
        assert_eq!(labels[0], "Type");
    }

    #[test]
    fn test_subfield_value_after_colon_space_with_existing_value() {
        let text = "Registry:\n  - Name: PyPI\n";
        let completions = get_completions(text, Position::new(1, 10));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels.len(), 17);
        assert_eq!(labels[0], "ASCL");
    }

    #[test]
    fn test_reference_type_value_after_colon_no_space() {
        let text = "Reference:\n  - Type:\n";
        let completions = get_completions(text, Position::new(1, 8));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels.len(), 8); // all reference types
        assert_eq!(labels[0], "Article");
    }

    #[test]
    fn test_reference_type_value_after_colon_space() {
        let text = "Reference:\n  - Type: \n";
        let completions = get_completions(text, Position::new(1, 10));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels.len(), 8);
        assert_eq!(labels[0], "Article");
    }

    #[test]
    fn test_subfield_insert_text_has_newline_and_indent() {
        let text = "Reference:\n  - Type: Book\n    \n";
        let completions = get_completions(text, Position::new(2, 4));
        let title = completions.iter().find(|c| c.label == "Title").unwrap();
        assert_eq!(title.insert_text_format, Some(InsertTextFormat::SNIPPET));
        assert_eq!(title.insert_text.as_deref(), Some("Title: $1\n    $0"));
    }

    #[test]
    fn test_value_insert_text_has_newline_and_indent() {
        let text = "Reference:\n  - Type: \n";
        let completions = get_completions(text, Position::new(1, 10));
        let article = completions.iter().find(|c| c.label == "Article").unwrap();
        assert_eq!(article.insert_text_format, Some(InsertTextFormat::SNIPPET));
        assert_eq!(article.insert_text.as_deref(), Some("Article\n    $0"));
    }
}
