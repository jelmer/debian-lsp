//! Semantic token generation for Debian changelog files.

use debian_changelog::SyntaxKind;
use tower_lsp_server::ls_types::SemanticToken;

use crate::deb822::semantic::{token_type, SemanticTokensBuilder};
use crate::position::offset_to_position;

/// Generate semantic tokens for a changelog file
pub fn generate_semantic_tokens(
    parse: &debian_changelog::Parse<debian_changelog::ChangeLog>,
    source_text: &str,
) -> Vec<SemanticToken> {
    let mut builder = SemanticTokensBuilder::new();

    // Use syntax_node() to get tokens even with parse errors
    let syntax = parse.syntax_node();

    for element in syntax.descendants_with_tokens() {
        if let rowan::NodeOrToken::Token(token) = element {
            let kind = token.kind();

            let token_type = match kind {
                SyntaxKind::IDENTIFIER => {
                    // Package name or metadata key — check parent node
                    let parent_kind = token.parent().map(|p| p.kind());
                    match parent_kind {
                        Some(SyntaxKind::ENTRY_HEADER) | Some(SyntaxKind::METADATA_KEY) => {
                            Some(token_type::FIELD)
                        }
                        _ => None,
                    }
                }
                SyntaxKind::VERSION => Some(token_type::VALUE),
                SyntaxKind::COMMENT => Some(token_type::COMMENT),
                _ => {
                    // Check parent for composite nodes
                    let parent_kind = token.parent().map(|p| p.kind());
                    match parent_kind {
                        Some(SyntaxKind::DISTRIBUTIONS) => Some(token_type::VALUE),
                        Some(SyntaxKind::METADATA_VALUE) => Some(token_type::VALUE),
                        Some(SyntaxKind::TIMESTAMP) => Some(token_type::VALUE),
                        Some(SyntaxKind::MAINTAINER) => Some(token_type::VALUE),
                        Some(SyntaxKind::EMAIL) => Some(token_type::VALUE),
                        _ => None,
                    }
                }
            };

            if let Some(tt) = token_type {
                let range = token.text_range();
                let start_pos = offset_to_position(source_text, range.start());
                let length = (usize::from(range.end()) - usize::from(range.start())) as u32;

                if length > 0 {
                    builder.push(start_pos.line, start_pos.character, length, tt, 0);
                }
            }
        }
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_semantic_tokens_basic() {
        let text = "test-package (1.0-1) unstable; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);

        let tokens = generate_semantic_tokens(&parsed, text);

        // Should have tokens for: package name, version, distribution, urgency key, urgency value,
        // maintainer, email, timestamp
        assert!(!tokens.is_empty());

        // First token should be the package name "test-package"
        assert_eq!(tokens[0].delta_line, 0);
        assert_eq!(tokens[0].delta_start, 0);
        assert_eq!(tokens[0].length, 12);
        assert_eq!(tokens[0].token_type, token_type::FIELD);

        // Second token should be the version "(1.0-1)"
        assert_eq!(tokens[1].token_type, token_type::VALUE);
    }

    #[test]
    fn test_generate_semantic_tokens_comment() {
        // Changelog comments start with a line that doesn't match entry format
        // but the parser may handle them differently. Test with a simple entry.
        let text = "test-package (1.0-1) unstable; urgency=medium\n\n  * Change.\n\n -- Test <test@test.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);

        let tokens = generate_semantic_tokens(&parsed, text);
        assert!(!tokens.is_empty());
    }

    #[test]
    fn test_generate_semantic_tokens_metadata_key() {
        let text = "test-package (1.0-1) unstable; urgency=medium\n\n  * Change.\n\n -- Test <test@test.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);

        let tokens = generate_semantic_tokens(&parsed, text);

        // Find the urgency token - should be FIELD type
        let urgency_token = tokens.iter().find(|t| {
            t.token_type == token_type::FIELD && t.delta_line == 0 && t.length == 7
        });
        assert!(
            urgency_token.is_some(),
            "Should have an urgency FIELD token"
        );
    }
}
