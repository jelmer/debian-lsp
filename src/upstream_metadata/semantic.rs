//! Semantic token generation for debian/upstream/metadata files.

use tower_lsp_server::ls_types::SemanticToken;
use yaml_edit::{Document, Mapping, YamlNode};

use super::fields::get_standard_field_name;
use crate::deb822::semantic::{SemanticTokensBuilder, TokenType};

/// Generate semantic tokens for a debian/upstream/metadata file.
pub fn generate_semantic_tokens(doc: &Document, source_text: &str) -> Vec<SemanticToken> {
    let mapping = match doc.as_mapping() {
        Some(m) => m,
        None => return vec![],
    };

    let mut builder = SemanticTokensBuilder::new();

    emit_mapping_tokens(&mapping, source_text, &mut builder, true);

    builder.build()
}

/// Emit semantic tokens for all entries in a mapping.
///
/// When `top_level` is true, keys are checked against DEP-12 field names to
/// distinguish known fields from unknown ones. Nested mapping keys are always
/// emitted as plain fields.
fn emit_mapping_tokens(
    mapping: &Mapping,
    source_text: &str,
    builder: &mut SemanticTokensBuilder,
    top_level: bool,
) {
    for entry in mapping.entries() {
        // Emit token for the key
        if let Some(YamlNode::Scalar(key_scalar)) = entry.key_node() {
            let key_text = key_scalar.as_string();
            let pos = key_scalar.start_position(source_text);
            let range = key_scalar.byte_range();
            let len = range.end - range.start;

            // LineColumn is 1-indexed, LSP is 0-indexed
            let line = pos.line.saturating_sub(1) as u32;
            let col = pos.column.saturating_sub(1) as u32;

            let token_type = if top_level {
                if get_standard_field_name(&key_text).is_some() {
                    TokenType::Field
                } else {
                    TokenType::UnknownField
                }
            } else {
                TokenType::Field
            };

            builder.push(
                line,
                col,
                len,
                token_type,
                crate::deb822::semantic::token_modifier::DECLARATION,
            );
        }

        // Emit tokens for the value, recursing into nested structures
        if let Some(value_node) = entry.value_node() {
            emit_node_tokens(&value_node, source_text, builder);
        }
    }
}

/// Emit semantic tokens for a YAML node, recursing into mappings and sequences.
fn emit_node_tokens(node: &YamlNode, source_text: &str, builder: &mut SemanticTokensBuilder) {
    match node {
        YamlNode::Scalar(scalar) => {
            let pos = scalar.start_position(source_text);
            let range = scalar.byte_range();
            let len = range.end - range.start;

            let line = pos.line.saturating_sub(1) as u32;
            let col = pos.column.saturating_sub(1) as u32;

            if len > 0 {
                builder.push(line, col, len, TokenType::Value, 0);
            }
        }
        YamlNode::Mapping(mapping) => {
            emit_mapping_tokens(mapping, source_text, builder, false);
        }
        YamlNode::Sequence(sequence) => {
            for item in sequence.values() {
                emit_node_tokens(&item, source_text, builder);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_doc(text: &str) -> Document {
        text.parse::<Document>().unwrap()
    }

    #[test]
    fn test_known_fields() {
        let text = "Repository: https://github.com/example/project\nBug-Database: https://github.com/example/project/issues\n";
        let doc = parse_doc(text);
        let tokens = generate_semantic_tokens(&doc, text);

        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
        assert_eq!(tokens[0].length, 10); // "Repository"
        assert_eq!(tokens[1].token_type, TokenType::Value as u32);
        assert_eq!(tokens[2].token_type, TokenType::Field as u32);
        assert_eq!(tokens[2].length, 12); // "Bug-Database"
        assert_eq!(tokens[3].token_type, TokenType::Value as u32);
    }

    #[test]
    fn test_unknown_field() {
        let text = "Repository: https://example.com\nX-Custom: value\n";
        let doc = parse_doc(text);
        let tokens = generate_semantic_tokens(&doc, text);

        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
        assert_eq!(tokens[2].token_type, TokenType::UnknownField as u32);
        assert_eq!(tokens[2].length, 8); // "X-Custom"
    }

    #[test]
    fn test_non_mapping_document() {
        // A YAML document that is a sequence, not a mapping
        let text = "- item1\n- item2\n";
        let doc = parse_doc(text);
        let tokens = generate_semantic_tokens(&doc, text);
        assert_eq!(tokens.len(), 0);
    }

    #[test]
    fn test_sequence_with_nested_mappings() {
        // Registry has a sequence of mappings — all keys and values should get tokens
        let text = "Registry:\n  - Name: PyPI\n    Entry: example\n";
        let doc = parse_doc(text);
        let tokens = generate_semantic_tokens(&doc, text);

        // Registry (key) + Name (key) + PyPI (value) + Entry (key) + example (value)
        assert_eq!(tokens.len(), 5);
        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
        assert_eq!(tokens[0].length, 8); // "Registry"
        assert_eq!(tokens[1].token_type, TokenType::Field as u32);
        assert_eq!(tokens[1].length, 4); // "Name"
        assert_eq!(tokens[2].token_type, TokenType::Value as u32);
        assert_eq!(tokens[2].length, 4); // "PyPI"
        assert_eq!(tokens[3].token_type, TokenType::Field as u32);
        assert_eq!(tokens[3].length, 5); // "Entry"
        assert_eq!(tokens[4].token_type, TokenType::Value as u32);
        assert_eq!(tokens[4].length, 7); // "example"
    }

    #[test]
    fn test_sequence_of_scalars() {
        let text = "Other-References:\n  - https://example.com\n  - https://example.org\n";
        let doc = parse_doc(text);
        let tokens = generate_semantic_tokens(&doc, text);

        // Other-References (key) + 2 scalar values
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
        assert_eq!(tokens[1].token_type, TokenType::Value as u32);
        assert_eq!(tokens[2].token_type, TokenType::Value as u32);
    }

    #[test]
    fn test_declaration_modifier_on_keys() {
        let text = "Repository: https://example.com\n";
        let doc = parse_doc(text);
        let tokens = generate_semantic_tokens(&doc, text);

        assert_eq!(tokens.len(), 2);
        // Key should have DECLARATION modifier
        assert_eq!(
            tokens[0].token_modifiers_bitset,
            crate::deb822::semantic::token_modifier::DECLARATION
        );
        // Value should have no modifiers
        assert_eq!(tokens[1].token_modifiers_bitset, 0);
    }

    #[test]
    fn test_delta_positions() {
        let text = "Repository: https://example.com\nBug-Database: https://bugs.example.com\n";
        let doc = parse_doc(text);
        let tokens = generate_semantic_tokens(&doc, text);

        assert_eq!(tokens.len(), 4);
        // First key at line 0
        assert_eq!(tokens[0].delta_line, 0);
        assert_eq!(tokens[0].delta_start, 0);
        // First value on same line
        assert_eq!(tokens[1].delta_line, 0);
        // Second key on next line
        assert_eq!(tokens[2].delta_line, 1);
        assert_eq!(tokens[2].delta_start, 0);
    }
}
