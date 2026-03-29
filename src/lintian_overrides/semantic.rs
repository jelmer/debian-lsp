use lintian_overrides::{AstNode as _, LintianOverrides, SyntaxKind};
use text_size::TextSize;
use tower_lsp_server::ls_types::SemanticToken;

use crate::position::offset_to_position;

/// Semantic token type indices matching the legend in main.rs:
/// 0 = debianField, 1 = debianUnknownField, 2 = debianValue,
/// 3 = debianComment
const TOKEN_TYPE_COMMENT: u32 = 3;
const TOKEN_TYPE_FIELD: u32 = 0;
const TOKEN_TYPE_VALUE: u32 = 2;

/// Generate semantic tokens for a lintian overrides file.
pub fn generate_semantic_tokens(
    overrides: &LintianOverrides,
    source_text: &str,
) -> Vec<SemanticToken> {
    let mut tokens = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for element in overrides.syntax().descendants_with_tokens() {
        let token = match element.into_token() {
            Some(t) => t,
            None => continue,
        };

        let token_type = match token.kind() {
            SyntaxKind::COMMENT => TOKEN_TYPE_COMMENT,
            SyntaxKind::TAG => TOKEN_TYPE_FIELD,
            SyntaxKind::PACKAGE_NAME | SyntaxKind::PACKAGE_TYPE => TOKEN_TYPE_VALUE,
            SyntaxKind::INFO => TOKEN_TYPE_VALUE,
            _ => continue,
        };

        let start_offset: TextSize = token.text_range().start();
        let length = token.text_range().len();
        let pos = offset_to_position(source_text, start_offset);

        let delta_line = pos.line - prev_line;
        let delta_start = if delta_line == 0 {
            pos.character - prev_start
        } else {
            pos.character
        };

        tokens.push(SemanticToken {
            delta_line,
            delta_start,
            length: u32::from(length),
            token_type,
            token_modifiers_bitset: 0,
        });

        prev_line = pos.line;
        prev_start = pos.character;
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantic_tokens_comment() {
        let text = "# This is a comment\n";
        let parsed = LintianOverrides::parse(text);
        let overrides = parsed.ok().unwrap();
        let tokens = generate_semantic_tokens(&overrides, text);

        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_type, TOKEN_TYPE_COMMENT);
    }

    #[test]
    fn test_semantic_tokens_simple_override() {
        let text = "some-tag\n";
        let parsed = LintianOverrides::parse(text);
        let overrides = parsed.ok().unwrap();
        let tokens = generate_semantic_tokens(&overrides, text);

        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_type, TOKEN_TYPE_FIELD);
    }

    #[test]
    fn test_semantic_tokens_override_with_package_and_info() {
        let text = "mypackage: some-tag extra info\n";
        let parsed = LintianOverrides::parse(text);
        let overrides = parsed.ok().unwrap();
        let tokens = generate_semantic_tokens(&overrides, text);

        // Should have tokens for: package name, tag, info
        assert!(tokens.len() >= 3);
    }
}
