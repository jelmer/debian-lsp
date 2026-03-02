//! Generic semantic token generation for deb822 files.
//!
//! This module provides the core logic for generating semantic tokens from
//! deb822-lossless parse trees. File-type-specific modules (control, copyright)
//! use this by providing field validation callbacks.

use deb822_lossless::{Deb822, SyntaxKind};
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::SemanticToken;

use crate::position::offset_to_position;

/// Token type indices (must match the order in main.rs initialize())
pub mod token_type {
    pub const FIELD: u32 = 0; // Known control field
    pub const UNKNOWN_FIELD: u32 = 1; // Unknown/custom field
    pub const VALUE: u32 = 2; // Field value
    pub const COMMENT: u32 = 3; // Comment
}

/// Token modifier indices
pub mod token_modifier {
    pub const DECLARATION: u32 = 1 << 0;
}

/// Field validation callback
pub trait FieldValidator {
    /// Check if a field name is valid and get its standard casing
    fn get_standard_field_name(&self, name: &str) -> Option<&'static str>;
}

/// Helper for building semantic token arrays
pub struct SemanticTokensBuilder {
    tokens: Vec<SemanticToken>,
    prev_line: u32,
    prev_char: u32,
}

impl SemanticTokensBuilder {
    pub fn new() -> Self {
        Self {
            tokens: Vec::new(),
            prev_line: 0,
            prev_char: 0,
        }
    }

    /// Add a token at the given position
    pub fn push(
        &mut self,
        line: u32,
        start_char: u32,
        length: u32,
        token_type: u32,
        token_modifiers: u32,
    ) {
        let delta_line = line - self.prev_line;
        let delta_start = if delta_line == 0 {
            start_char - self.prev_char
        } else {
            start_char
        };

        self.tokens.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type,
            token_modifiers_bitset: token_modifiers,
        });

        self.prev_line = line;
        self.prev_char = start_char;
    }

    pub fn build(self) -> Vec<SemanticToken> {
        self.tokens
    }
}

impl Default for SemanticTokensBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate semantic tokens for a deb822 file
pub fn generate_tokens<V: FieldValidator>(
    deb822: &Deb822,
    source_text: &str,
    validator: &V,
) -> Vec<SemanticToken> {
    let mut builder = SemanticTokensBuilder::new();

    // Single pass through the syntax tree
    for element in deb822.syntax().descendants_with_tokens() {
        if let rowan::NodeOrToken::Token(token) = element {
            match token.kind() {
                SyntaxKind::COMMENT => {
                    let range = token.text_range();
                    let start_pos = offset_to_position(source_text, range.start());
                    let length = (usize::from(range.end()) - usize::from(range.start())) as u32;

                    builder.push(
                        start_pos.line,
                        start_pos.character,
                        length,
                        token_type::COMMENT,
                        0,
                    );
                }
                SyntaxKind::KEY => {
                    let range = token.text_range();
                    let start_pos = offset_to_position(source_text, range.start());
                    let key = token.text();
                    let length = key.len() as u32;

                    // Check if field is known
                    let token_type = if validator.get_standard_field_name(key).is_some() {
                        token_type::FIELD
                    } else {
                        token_type::UNKNOWN_FIELD
                    };

                    builder.push(
                        start_pos.line,
                        start_pos.character,
                        length,
                        token_type,
                        token_modifier::DECLARATION,
                    );
                }
                SyntaxKind::VALUE => {
                    let range = token.text_range();
                    let start_pos = offset_to_position(source_text, range.start());
                    let length = (usize::from(range.end()) - usize::from(range.start())) as u32;

                    if length > 0 {
                        builder.push(
                            start_pos.line,
                            start_pos.character,
                            length,
                            token_type::VALUE,
                            0,
                        );
                    }
                }
                _ => {}
            }
        }
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestValidator;
    impl FieldValidator for TestValidator {
        fn get_standard_field_name(&self, name: &str) -> Option<&'static str> {
            if name.eq_ignore_ascii_case("Source") {
                Some("Source")
            } else if name.eq_ignore_ascii_case("Package") {
                Some("Package")
            } else {
                None
            }
        }
    }

    #[test]
    fn test_semantic_tokens_builder() {
        let mut builder = SemanticTokensBuilder::new();

        // Add a token on line 0
        builder.push(0, 0, 6, token_type::FIELD, 0);

        // Add another token on the same line
        builder.push(0, 8, 4, token_type::VALUE, 0);

        // Add a token on line 1
        builder.push(1, 0, 7, token_type::FIELD, 0);

        let tokens = builder.build();
        assert_eq!(tokens.len(), 3);

        // First token
        assert_eq!(tokens[0].delta_line, 0);
        assert_eq!(tokens[0].delta_start, 0);
        assert_eq!(tokens[0].length, 6);

        // Second token (same line)
        assert_eq!(tokens[1].delta_line, 0);
        assert_eq!(tokens[1].delta_start, 8);

        // Third token (new line)
        assert_eq!(tokens[2].delta_line, 1);
        assert_eq!(tokens[2].delta_start, 0);
    }
}
