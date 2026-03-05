//! Semantic token generation for Debian changelog files.

use debian_changelog::SyntaxKind;
use tower_lsp_server::ls_types::SemanticToken;

use crate::deb822::semantic::{SemanticTokensBuilder, TokenType};
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
                    let parent_kind = token.parent().map(|p| p.kind());
                    match parent_kind {
                        Some(SyntaxKind::ENTRY_HEADER) => Some(TokenType::ChangelogPackage),
                        Some(SyntaxKind::METADATA_KEY) => Some(TokenType::ChangelogUrgency),
                        Some(SyntaxKind::DISTRIBUTIONS) => Some(TokenType::ChangelogDistribution),
                        _ => None,
                    }
                }
                SyntaxKind::VERSION => Some(TokenType::ChangelogVersion),
                SyntaxKind::COMMENT => Some(TokenType::Comment),
                _ => {
                    let parent_kind = token.parent().map(|p| p.kind());
                    match parent_kind {
                        Some(SyntaxKind::METADATA_VALUE) => Some(TokenType::Value),
                        Some(SyntaxKind::TIMESTAMP) => Some(TokenType::ChangelogTimestamp),
                        Some(SyntaxKind::MAINTAINER) => Some(TokenType::ChangelogMaintainer),
                        Some(SyntaxKind::EMAIL) => Some(TokenType::ChangelogMaintainer),
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
        assert!(!tokens.is_empty());

        // First token: package name "test-package"
        assert_eq!(tokens[0].delta_line, 0);
        assert_eq!(tokens[0].delta_start, 0);
        assert_eq!(tokens[0].length, 12);
        assert_eq!(tokens[0].token_type, TokenType::ChangelogPackage as u32);

        // Second token: version "(1.0-1)"
        assert_eq!(tokens[1].token_type, TokenType::ChangelogVersion as u32);
    }

    #[test]
    fn test_generate_semantic_tokens_distribution() {
        let text = "test-package (1.0-1) unstable; urgency=medium\n\n  * Change.\n\n -- Test <test@test.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);

        let tokens = generate_semantic_tokens(&parsed, text);

        let dist_token = tokens
            .iter()
            .find(|t| t.token_type == TokenType::ChangelogDistribution as u32);
        assert!(dist_token.is_some(), "Should have a distribution token");
    }

    #[test]
    fn test_generate_semantic_tokens_urgency() {
        let text = "test-package (1.0-1) unstable; urgency=medium\n\n  * Change.\n\n -- Test <test@test.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);

        let tokens = generate_semantic_tokens(&parsed, text);

        let urgency_token = tokens
            .iter()
            .find(|t| t.token_type == TokenType::ChangelogUrgency as u32 && t.length == 7);
        assert!(urgency_token.is_some(), "Should have an urgency token");
    }

    #[test]
    fn test_generate_semantic_tokens_maintainer_and_timestamp() {
        let text = "test-package (1.0-1) unstable; urgency=medium\n\n  * Change.\n\n -- Test <test@test.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);

        let tokens = generate_semantic_tokens(&parsed, text);

        let has_maintainer = tokens
            .iter()
            .any(|t| t.token_type == TokenType::ChangelogMaintainer as u32);
        assert!(has_maintainer, "Should have a maintainer token");

        let has_timestamp = tokens
            .iter()
            .any(|t| t.token_type == TokenType::ChangelogTimestamp as u32);
        assert!(has_timestamp, "Should have a timestamp token");
    }
}
