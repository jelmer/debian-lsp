//! Semantic token generation for debian/upstream/metadata files.

use tower_lsp_server::ls_types::SemanticToken;
use yaml_edit::{Document, YamlNode};

use super::fields::get_standard_field_name;
use crate::deb822::semantic::{SemanticTokensBuilder, TokenType};

/// Generate semantic tokens for a debian/upstream/metadata file.
pub fn generate_semantic_tokens(source_text: &str) -> Vec<SemanticToken> {
    let doc = match source_text.parse::<Document>() {
        Ok(doc) => doc,
        Err(_) => return vec![],
    };

    let mapping = match doc.as_mapping() {
        Some(m) => m,
        None => return vec![],
    };

    let mut builder = SemanticTokensBuilder::new();

    for entry in mapping.entries() {
        // Emit token for the key
        if let Some(YamlNode::Scalar(key_scalar)) = entry.key_node() {
            let key_text = key_scalar.value();
            let pos = key_scalar.start_position(source_text);
            let len = key_text.len() as u32;

            // LineColumn is 1-indexed, LSP is 0-indexed
            let line = pos.line.saturating_sub(1) as u32;
            let col = pos.column.saturating_sub(1) as u32;

            let token_type = if get_standard_field_name(&key_text).is_some() {
                TokenType::Field
            } else {
                TokenType::UnknownField
            };

            builder.push(
                line,
                col,
                len,
                token_type,
                crate::deb822::semantic::token_modifier::DECLARATION,
            );
        }

        // Emit token for the value (only for scalar values)
        if let Some(YamlNode::Scalar(val_scalar)) = entry.value_node() {
            let pos = val_scalar.start_position(source_text);
            let range = val_scalar.byte_range();
            let len = range.end - range.start;

            let line = pos.line.saturating_sub(1) as u32;
            let col = pos.column.saturating_sub(1) as u32;

            if len > 0 {
                builder.push(line, col, len, TokenType::Value, 0);
            }
        }
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_fields() {
        let text = "Repository: https://github.com/example/project\nBug-Database: https://github.com/example/project/issues\n";
        let tokens = generate_semantic_tokens(text);

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
        let tokens = generate_semantic_tokens(text);

        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
        assert_eq!(tokens[2].token_type, TokenType::UnknownField as u32);
        assert_eq!(tokens[2].length, 8); // "X-Custom"
    }

    #[test]
    fn test_empty_text() {
        let tokens = generate_semantic_tokens("");
        assert_eq!(tokens.len(), 0);
    }

    #[test]
    fn test_invalid_yaml() {
        let tokens = generate_semantic_tokens("{{invalid yaml");
        assert_eq!(tokens.len(), 0);
    }
}
