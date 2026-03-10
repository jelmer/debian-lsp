//! Semantic token generation for Debian watch files.
//!
//! Supports both deb822 (v5) and line-based (v1-4) watch file formats.

use tower_lsp_server::ls_types::SemanticToken;

use crate::deb822::semantic::{SemanticTokensBuilder, TokenType};
use crate::position::offset_to_position;

/// Field validator for v5 watch files
struct WatchFieldValidator;

impl crate::deb822::semantic::FieldValidator for WatchFieldValidator {
    fn get_standard_field_name(&self, name: &str) -> Option<&'static str> {
        super::fields::get_standard_field_name(name)
    }
}

/// Generate semantic tokens for a watch file
pub fn generate_semantic_tokens(
    parse: &debian_watch::parse::Parse,
    source_text: &str,
) -> Vec<SemanticToken> {
    match parse.to_watch_file() {
        debian_watch::parse::ParsedWatchFile::Deb822(_) => generate_deb822_tokens(source_text),
        debian_watch::parse::ParsedWatchFile::LineBased(_) => {
            generate_linebased_tokens(source_text)
        }
    }
}

/// Generate tokens for v5 deb822 watch files
fn generate_deb822_tokens(source_text: &str) -> Vec<SemanticToken> {
    let deb822_parse = deb822_lossless::Deb822::parse(source_text);
    let deb822 = deb822_parse.tree();
    let validator = WatchFieldValidator;
    crate::deb822::semantic::generate_tokens(&deb822, source_text, &validator)
}

/// Generate tokens for v1-4 line-based watch files
fn generate_linebased_tokens(source_text: &str) -> Vec<SemanticToken> {
    use debian_watch::SyntaxKind;

    let parsed = debian_watch::linebased::parse_watch_file(source_text);
    let wf = parsed.tree();
    let mut builder = SemanticTokensBuilder::new();

    for element in wf.syntax().descendants_with_tokens() {
        if let rowan::NodeOrToken::Token(token) = element {
            let kind = token.kind();

            let token_type = match kind {
                SyntaxKind::KEY => Some(TokenType::Field),
                SyntaxKind::VALUE => {
                    let parent_kind = token.parent().map(|p| p.kind());
                    match parent_kind {
                        Some(SyntaxKind::VERSION)
                        | Some(SyntaxKind::OPTION)
                        | Some(SyntaxKind::URL)
                        | Some(SyntaxKind::MATCHING_PATTERN)
                        | Some(SyntaxKind::VERSION_POLICY)
                        | Some(SyntaxKind::SCRIPT) => Some(TokenType::Value),
                        _ => None,
                    }
                }
                SyntaxKind::COMMENT => Some(TokenType::Comment),
                _ => None,
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
    fn test_v5_known_fields() {
        let text =
            "Version: 5\n\nSource: https://github.com/owner/repo/tags\nMatching-Pattern: .*/v?(\\d[\\d.]*)/.tar.gz\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let tokens = generate_semantic_tokens(&parsed, text);

        assert!(!tokens.is_empty());

        // "Version" is a known field
        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
        assert_eq!(tokens[0].length, 7);
    }

    #[test]
    fn test_v5_unknown_field() {
        let text = "Version: 5\n\nSource: https://example.com\nX-Custom: value\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let tokens = generate_semantic_tokens(&parsed, text);

        let unknown = tokens
            .iter()
            .find(|t| t.token_type == TokenType::UnknownField as u32);
        assert!(unknown.is_some(), "Should have an unknown field token");
    }

    #[test]
    fn test_v4_produces_tokens() {
        let text = "version=4\nhttps://example.com/files .*/foo-(\\d[\\d.]*)/.tar\\.gz\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let tokens = generate_semantic_tokens(&parsed, text);

        assert!(!tokens.is_empty(), "v4 watch files should produce tokens");

        // Should have a field token for "version"
        let has_field = tokens
            .iter()
            .any(|t| t.token_type == TokenType::Field as u32);
        assert!(has_field, "Should have a field token");
    }

    #[test]
    fn test_v4_comment() {
        let text =
            "version=4\n# This is a comment\nhttps://example.com .*/foo-(\\d[\\d.]*)/.tar\\.gz\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let tokens = generate_semantic_tokens(&parsed, text);

        let has_comment = tokens
            .iter()
            .any(|t| t.token_type == TokenType::Comment as u32);
        assert!(has_comment, "Should have a comment token");
    }

    #[test]
    fn test_v4_url_and_pattern() {
        let text = "version=4\nhttps://example.com/files .*/foo-(\\d[\\d.]*)/.tar\\.gz\n";
        let parsed = debian_watch::parse::Parse::parse(text);
        let tokens = generate_semantic_tokens(&parsed, text);

        let value_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::Value as u32)
            .collect();
        assert!(
            value_tokens.len() >= 2,
            "Should have value tokens for version number, URL and/or pattern"
        );
    }

    #[test]
    fn test_field_validator() {
        let validator = WatchFieldValidator;
        use crate::deb822::semantic::FieldValidator;

        assert_eq!(validator.get_standard_field_name("Source"), Some("Source"));
        assert_eq!(validator.get_standard_field_name("source"), Some("Source"));
        assert_eq!(
            validator.get_standard_field_name("Matching-Pattern"),
            Some("Matching-Pattern")
        );
        assert_eq!(validator.get_standard_field_name("UnknownField"), None);
    }
}
