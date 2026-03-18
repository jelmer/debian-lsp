//! Semantic token generation for debian/rules files.

use makefile_lossless::{Makefile, SyntaxKind};
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::SemanticToken;

use super::fields::is_known_target;
use crate::deb822::semantic::{SemanticTokensBuilder, TokenType};
use crate::position::{offset_to_position, utf16_len};

/// Generate semantic tokens for a debian/rules file.
pub fn generate_semantic_tokens(makefile: &Makefile, source_text: &str) -> Vec<SemanticToken> {
    let mut builder = SemanticTokensBuilder::new();

    for element in makefile.syntax().descendants_with_tokens() {
        if let rowan::NodeOrToken::Token(token) = element {
            match token.kind() {
                SyntaxKind::COMMENT => {
                    let range = token.text_range();
                    let start_pos = offset_to_position(source_text, range.start());
                    let length = utf16_len(token.text());
                    builder.push(
                        start_pos.line,
                        start_pos.character,
                        length,
                        TokenType::Comment,
                        0,
                    );
                }
                SyntaxKind::IDENTIFIER => {
                    // Check parent node to determine context
                    if let Some(parent) = token.parent() {
                        let range = token.text_range();
                        let start_pos = offset_to_position(source_text, range.start());
                        let length = utf16_len(token.text());
                        let text = token.text();

                        match parent.kind() {
                            SyntaxKind::TARGETS => {
                                let token_type = if is_known_target(text) {
                                    TokenType::Field
                                } else {
                                    TokenType::UnknownField
                                };
                                builder.push(
                                    start_pos.line,
                                    start_pos.character,
                                    length,
                                    token_type,
                                    crate::deb822::semantic::token_modifier::DECLARATION,
                                );
                            }
                            SyntaxKind::VARIABLE => {
                                builder.push(
                                    start_pos.line,
                                    start_pos.character,
                                    length,
                                    TokenType::Field,
                                    crate::deb822::semantic::token_modifier::DECLARATION,
                                );
                            }
                            _ => {}
                        }
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

    #[test]
    fn test_known_target() {
        let text = "clean:\n\trm -rf build\n";
        let parsed = Makefile::parse(text);
        let makefile = parsed.tree();
        let tokens = generate_semantic_tokens(&makefile, text);

        assert!(!tokens.is_empty());
        // "clean" is a known target
        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
    }

    #[test]
    fn test_unknown_target() {
        let text = "my-custom-target:\n\techo hello\n";
        let parsed = Makefile::parse(text);
        let makefile = parsed.tree();
        let tokens = generate_semantic_tokens(&makefile, text);

        assert!(!tokens.is_empty());
        // "my-custom-target" is not a known target
        assert_eq!(tokens[0].token_type, TokenType::UnknownField as u32);
    }

    #[test]
    fn test_variable_definition() {
        let text = "PYTHON = python3\n";
        let parsed = Makefile::parse(text);
        let makefile = parsed.tree();
        let tokens = generate_semantic_tokens(&makefile, text);

        assert!(!tokens.is_empty());
        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
        assert_eq!(
            tokens[0].token_modifiers_bitset,
            crate::deb822::semantic::token_modifier::DECLARATION
        );
    }

    #[test]
    fn test_comment() {
        let text = "# This is a comment\nclean:\n\trm -rf build\n";
        let parsed = Makefile::parse(text);
        let makefile = parsed.tree();
        let tokens = generate_semantic_tokens(&makefile, text);

        assert!(!tokens.is_empty());
        assert_eq!(tokens[0].token_type, TokenType::Comment as u32);
    }

    #[test]
    fn test_empty_file() {
        let text = "";
        let parsed = Makefile::parse(text);
        let makefile = parsed.tree();
        let tokens = generate_semantic_tokens(&makefile, text);
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_override_target() {
        let text = "override_dh_auto_build:\n\tdh_auto_build -- --verbose\n";
        let parsed = Makefile::parse(text);
        let makefile = parsed.tree();
        let tokens = generate_semantic_tokens(&makefile, text);

        assert!(!tokens.is_empty());
        // override_dh_auto_build is a known target
        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
    }
}
