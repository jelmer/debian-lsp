//! Semantic token generation for Debian control files.

use tower_lsp_server::ls_types::SemanticToken;

use super::get_standard_field_name;
use crate::deb822::semantic::{generate_tokens, FieldValidator};

/// Field validator for control files
pub struct ControlFieldValidator;

impl FieldValidator for ControlFieldValidator {
    fn get_standard_field_name(&self, name: &str) -> Option<&'static str> {
        get_standard_field_name(name)
    }
}

/// Generate semantic tokens for a control file
pub fn generate_semantic_tokens(
    control: &debian_control::lossless::Control,
    source_text: &str,
) -> Vec<SemanticToken> {
    let validator = ControlFieldValidator;
    generate_tokens(control.as_deb822(), source_text, &validator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deb822::semantic::TokenType;

    #[test]
    fn test_generate_semantic_tokens_known_fields() {
        let text = "Source: test-package\nMaintainer: Test User <test@example.com>\n";
        let parsed = debian_control::lossless::Control::parse(text);

        let control = parsed.to_result().expect("Should parse");
        let tokens = generate_semantic_tokens(&control, text);

        // Should have 4 tokens: Source (field), test-package (value), Maintainer (field), value
        assert_eq!(tokens.len(), 4, "Should have exactly 4 tokens");

        // First token: "Source" field name at line 0, char 0
        assert_eq!(tokens[0].delta_line, 0, "Source should be on line 0");
        assert_eq!(tokens[0].delta_start, 0, "Source should start at char 0");
        assert_eq!(tokens[0].length, 6, "Source length should be 6");
        assert_eq!(
            tokens[0].token_type,
            TokenType::Field as u32,
            "Source should be classified as Field (known field)"
        );

        // Second token: "test-package" value at line 0, char 8
        assert_eq!(tokens[1].delta_line, 0, "Value should be on same line");
        assert!(
            tokens[1].delta_start > 0,
            "Value should be after field name"
        );
        assert_eq!(
            tokens[1].token_type,
            TokenType::Value as u32,
            "Value should be classified as Value"
        );

        // Third token: "Maintainer" field name at line 1, char 0
        assert_eq!(tokens[2].delta_line, 1, "Maintainer should be on line 1");
        assert_eq!(
            tokens[2].delta_start, 0,
            "Maintainer should start at char 0"
        );
        assert_eq!(tokens[2].length, 10, "Maintainer length should be 10");
        assert_eq!(
            tokens[2].token_type,
            TokenType::Field as u32,
            "Maintainer should be classified as Field (known field)"
        );

        // Fourth token: maintainer value
        assert_eq!(tokens[3].delta_line, 0, "Value should be on same line");
        assert_eq!(
            tokens[3].token_type,
            TokenType::Value as u32,
            "Value should be classified as Value"
        );
    }

    #[test]
    fn test_generate_semantic_tokens_unknown_field() {
        let text = "Source: test\nX-Custom-Field: value\n";
        let parsed = debian_control::lossless::Control::parse(text);

        let control = parsed.to_result().expect("Should parse");
        let tokens = generate_semantic_tokens(&control, text);

        assert_eq!(tokens.len(), 4, "Should have 4 tokens");

        // First token: "Source" - known field
        assert_eq!(
            tokens[0].token_type,
            TokenType::Field as u32,
            "Source should be Field (known)"
        );

        // Third token: "X-Custom-Field" - unknown field
        assert_eq!(
            tokens[2].token_type,
            TokenType::UnknownField as u32,
            "Unknown field should be classified as UnknownField"
        );
        assert_eq!(tokens[2].length, 14, "X-Custom-Field length should be 14");
    }

    #[test]
    fn test_generate_semantic_tokens_case_insensitive() {
        let text = "source: test\n";
        let parsed = debian_control::lossless::Control::parse(text);

        let control = parsed.to_result().expect("Should parse");
        let tokens = generate_semantic_tokens(&control, text);

        // "source" (lowercase) should still be recognized as a known field
        assert_eq!(
            tokens[0].token_type,
            TokenType::Field as u32,
            "Lowercase 'source' should still be classified as Field"
        );
    }

    #[test]
    fn test_field_validator() {
        let validator = ControlFieldValidator;

        // Known fields
        assert_eq!(validator.get_standard_field_name("Source"), Some("Source"));
        assert_eq!(validator.get_standard_field_name("source"), Some("Source"));
        assert_eq!(
            validator.get_standard_field_name("Package"),
            Some("Package")
        );

        // Unknown field
        assert_eq!(validator.get_standard_field_name("UnknownField"), None);
    }
}
