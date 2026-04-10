//! Semantic token generation for debian/patches/series files.
use crate::deb822::semantic::{token_modifier, SemanticTokensBuilder, TokenType};
use crate::position::offset_to_position;
use patchkit::edit::series::lex::SyntaxKind;
use patchkit::edit::series::lossless::SeriesFile;
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::SemanticToken;

/// Generate semantic tokens for a debian/patches/series file
pub fn generate_semantic_tokens(series: &SeriesFile, source_text: &str) -> Vec<SemanticToken> {
    let mut builder = SemanticTokensBuilder::new();

    for element in series.syntax().descendants_with_tokens() {
        if let rowan::NodeOrToken::Token(token) = element {
            match token.kind() {
                SyntaxKind::HASH | SyntaxKind::TEXT => {
                    let range = token.text_range();
                    let start_pos = offset_to_position(source_text, range.start());
                    let length = crate::position::utf16_len(token.text());
                    builder.push(
                        start_pos.line,
                        start_pos.character,
                        length,
                        TokenType::Comment,
                        0,
                    );
                }
                SyntaxKind::PATCH_NAME => {
                    let range = token.text_range();
                    let start_pos = offset_to_position(source_text, range.start());
                    let length = crate::position::utf16_len(token.text());
                    builder.push(
                        start_pos.line,
                        start_pos.character,
                        length,
                        TokenType::Value,
                        token_modifier::DECLARATION,
                    );
                }
                SyntaxKind::OPTION => {
                    let range = token.text_range();
                    let start_pos = offset_to_position(source_text, range.start());
                    let length = crate::position::utf16_len(token.text());
                    builder.push(
                        start_pos.line,
                        start_pos.character,
                        length,
                        TokenType::Field,
                        0,
                    );
                }
                _ => {}
            }
        }
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::generate_semantic_tokens;
    use crate::deb822::semantic::TokenType;

    #[test]
    fn test_generate_semantic_tokens_patch_name() {
        let text = "fix-arm.patch\n";
        let parsed = patchkit::edit::series::parse(text);
        let series = parsed.tree();
        let tokens = generate_semantic_tokens(&series, text);

        assert!(!tokens.is_empty());
        assert_eq!(tokens[0].delta_line, 0);
        assert_eq!(tokens[0].delta_start, 0);
        assert_eq!(tokens[0].length, 13);
        assert_eq!(tokens[0].token_type, TokenType::Value as u32);
    }

    #[test]
    fn test_generate_semantic_tokens_patch_with_option() {
        let text = "fix-arm.patch -p1\n";
        let parsed = patchkit::edit::series::parse(text);
        let series = parsed.tree();
        let tokens = generate_semantic_tokens(&series, text);

        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].token_type, TokenType::Value as u32);
        assert_eq!(tokens[0].length, 13);
        assert_eq!(tokens[1].token_type, TokenType::Field as u32);
        assert_eq!(tokens[1].length, 3);
    }

    #[test]
    fn test_generate_semantic_tokens_comment() {
        let text = "# This is a comment\n";
        let parsed = patchkit::edit::series::parse(text);
        let series = parsed.tree();
        let tokens = generate_semantic_tokens(&series, text);

        assert!(!tokens.is_empty());
        for token in &tokens {
            assert_eq!(token.token_type, TokenType::Comment as u32);
        }
    }

    #[test]
    fn test_generate_semantic_tokens_multiple_patches() {
        let text = "fix-arm.patch\nfix-mips.patch -p1\n";
        let parsed = patchkit::edit::series::parse(text);
        let series = parsed.tree();
        let tokens = generate_semantic_tokens(&series, text);

        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].token_type, TokenType::Value as u32);
        assert_eq!(tokens[1].token_type, TokenType::Value as u32);
        assert_eq!(tokens[2].token_type, TokenType::Field as u32);
    }

    #[test]
    fn test_generate_semantic_tokens_empty_file() {
        let text = "";
        let parsed = patchkit::edit::series::parse(text);
        let series = parsed.tree();
        let tokens = generate_semantic_tokens(&series, text);

        assert!(tokens.is_empty());
    }

    #[test]
    fn test_generate_semantic_tokens_mixed() {
        let text = "# Security\nfix-arm.patch -p1\nooo.patch\n";
        let parsed = patchkit::edit::series::parse(text);
        let series = parsed.tree();
        let tokens = generate_semantic_tokens(&series, text);

        assert!(!tokens.is_empty());
        assert_eq!(tokens[0].token_type, TokenType::Comment as u32);
    }

    #[test]
    fn test_generate_semantic_tokens_subdir_patch() {
        let text = "upstream/fix-arm.patch\n";
        let parsed = patchkit::edit::series::parse(text);
        let series = parsed.tree();
        let tokens = generate_semantic_tokens(&series, text);

        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_type, TokenType::Value as u32);
        assert_eq!(tokens[0].length, 22);
    }
}
