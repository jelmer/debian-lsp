//! Semantic token generation for Debian copyright files.

use tower_lsp_server::ls_types::SemanticToken;

use super::get_standard_field_name;
use crate::deb822::semantic::{generate_tokens, FieldValidator};

/// Field validator for copyright files
pub struct CopyrightFieldValidator;

impl FieldValidator for CopyrightFieldValidator {
    fn get_standard_field_name(&self, name: &str) -> Option<&'static str> {
        get_standard_field_name(name)
    }
}

/// Generate semantic tokens for a copyright file
pub fn generate_semantic_tokens(
    copyright: &debian_copyright::lossless::Copyright,
    source_text: &str,
) -> Vec<SemanticToken> {
    let validator = CopyrightFieldValidator;
    generate_tokens(copyright.as_deb822(), source_text, &validator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deb822::semantic::TokenType;

    #[test]
    fn test_generate_semantic_tokens_known_fields() {
        let text = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test\n";
        let parsed = debian_copyright::lossless::Parse::parse(text);

        let copyright = parsed.tree();
        let tokens = generate_semantic_tokens(&copyright, text);

        assert_eq!(tokens.len(), 4, "Should have exactly 4 tokens");

        // First token: "Format" field name
        assert_eq!(tokens[0].delta_line, 0);
        assert_eq!(tokens[0].delta_start, 0);
        assert_eq!(tokens[0].length, 6);
        assert_eq!(tokens[0].token_type, TokenType::Field as u32);

        // Second token: format value
        assert_eq!(tokens[1].delta_line, 0);
        assert_eq!(tokens[1].token_type, TokenType::Value as u32);

        // Third token: "Upstream-Name" field name
        assert_eq!(tokens[2].delta_line, 1);
        assert_eq!(tokens[2].delta_start, 0);
        assert_eq!(tokens[2].length, 13);
        assert_eq!(tokens[2].token_type, TokenType::Field as u32);

        // Fourth token: upstream-name value
        assert_eq!(tokens[3].delta_line, 0);
        assert_eq!(tokens[3].token_type, TokenType::Value as u32);
    }

    #[test]
    fn test_generate_semantic_tokens_unknown_field() {
        let text = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nX-Custom: value\n";
        let parsed = debian_copyright::lossless::Parse::parse(text);

        let copyright = parsed.tree();
        let tokens = generate_semantic_tokens(&copyright, text);

        assert_eq!(tokens.len(), 4);

        // First token: "Format" - known field
        assert_eq!(tokens[0].token_type, TokenType::Field as u32);

        // Third token: "X-Custom" - unknown field
        assert_eq!(tokens[2].token_type, TokenType::UnknownField as u32);
        assert_eq!(tokens[2].length, 8);
    }

    #[test]
    fn test_generate_semantic_tokens_case_insensitive() {
        let text = "format: https://example.com\n";
        let parsed = debian_copyright::lossless::Parse::parse(text);

        let copyright = parsed.tree();
        let tokens = generate_semantic_tokens(&copyright, text);

        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
    }

    #[test]
    fn test_field_validator() {
        let validator = CopyrightFieldValidator;

        assert_eq!(validator.get_standard_field_name("Format"), Some("Format"));
        assert_eq!(validator.get_standard_field_name("format"), Some("Format"));
        assert_eq!(validator.get_standard_field_name("Files"), Some("Files"));
        assert_eq!(
            validator.get_standard_field_name("License"),
            Some("License")
        );
        assert_eq!(validator.get_standard_field_name("UnknownField"), None);
    }
}
