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
                        Some(SyntaxKind::METADATA_VALUE) => Some(TokenType::ChangelogMetadataValue),
                        _ => None,
                    }
                }
                SyntaxKind::VERSION => Some(TokenType::ChangelogVersion),
                SyntaxKind::COMMENT => Some(TokenType::Comment),
                _ => {
                    let parent_kind = token.parent().map(|p| p.kind());
                    match parent_kind {
                        Some(SyntaxKind::METADATA_VALUE) => Some(TokenType::ChangelogMetadataValue),
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

    /// Helper to collect (token_type, length) pairs for easier assertions
    fn token_summary(tokens: &[SemanticToken]) -> Vec<(u32, u32)> {
        tokens.iter().map(|t| (t.token_type, t.length)).collect()
    }

    #[test]
    fn test_all_token_types_in_entry() {
        let text = "test-package (1.0-1) unstable; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);
        let tokens = generate_semantic_tokens(&parsed, text);

        let summary = token_summary(&tokens);

        // Verify we see all expected token types
        let types: Vec<u32> = summary.iter().map(|(tt, _)| *tt).collect();

        assert!(
            types.contains(&(TokenType::ChangelogPackage as u32)),
            "Missing ChangelogPackage in {types:?}"
        );
        assert!(
            types.contains(&(TokenType::ChangelogVersion as u32)),
            "Missing ChangelogVersion in {types:?}"
        );
        assert!(
            types.contains(&(TokenType::ChangelogDistribution as u32)),
            "Missing ChangelogDistribution in {types:?}"
        );
        assert!(
            types.contains(&(TokenType::ChangelogUrgency as u32)),
            "Missing ChangelogUrgency in {types:?}"
        );
        assert!(
            types.contains(&(TokenType::ChangelogMetadataValue as u32)),
            "Missing Value (metadata value) in {types:?}"
        );
        assert!(
            types.contains(&(TokenType::ChangelogMaintainer as u32)),
            "Missing ChangelogMaintainer in {types:?}"
        );
        assert!(
            types.contains(&(TokenType::ChangelogTimestamp as u32)),
            "Missing ChangelogTimestamp in {types:?}"
        );

        // First token should be the package name
        assert_eq!(tokens[0].delta_line, 0);
        assert_eq!(tokens[0].delta_start, 0);
        assert_eq!(tokens[0].token_type, TokenType::ChangelogPackage as u32);
        assert_eq!(tokens[0].length, 12);
    }

    #[test]
    fn test_maintainer_and_timestamp() {
        let text = "pkg (1.0-1) unstable; urgency=low\n\n  * Change.\n\n -- Test User <test@test.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);
        let tokens = generate_semantic_tokens(&parsed, text);

        let summary = token_summary(&tokens);

        let has_maintainer = summary
            .iter()
            .any(|(tt, _)| *tt == TokenType::ChangelogMaintainer as u32);
        assert!(has_maintainer, "Should have a maintainer token");

        let has_timestamp = summary
            .iter()
            .any(|(tt, _)| *tt == TokenType::ChangelogTimestamp as u32);
        assert!(has_timestamp, "Should have a timestamp token");
    }

    #[test]
    fn test_multiple_entries() {
        let text = "\
pkg (2.0-1) unstable; urgency=medium

  * Second release.

 -- A <a@example.com>  Mon, 01 Jan 2025 12:00:00 +0000

pkg (1.0-1) unstable; urgency=low

  * First release.

 -- B <b@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
";
        let parsed = debian_changelog::ChangeLog::parse(text);
        let tokens = generate_semantic_tokens(&parsed, text);

        // Should have package tokens for both entries
        let package_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::ChangelogPackage as u32)
            .collect();
        assert_eq!(package_tokens.len(), 2, "Should have 2 package name tokens");

        // Should have version tokens for both entries
        let version_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::ChangelogVersion as u32)
            .collect();
        assert_eq!(version_tokens.len(), 2, "Should have 2 version tokens");
    }

    #[test]
    fn test_multiple_distributions() {
        let text = "pkg (1.0-1) unstable testing; urgency=low\n\n  * Change.\n\n -- T <t@t.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);
        let tokens = generate_semantic_tokens(&parsed, text);

        let dist_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::ChangelogDistribution as u32)
            .collect();
        assert_eq!(
            dist_tokens.len(),
            2,
            "Should have 2 distribution tokens for 'unstable testing'"
        );
    }
}
