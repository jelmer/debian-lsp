//! Semantic token generation for debian/tests/control files.

use tower_lsp_server::ls_types::SemanticToken;

use crate::deb822::semantic::{generate_tokens, FieldValidator};

/// Known field names for debian/tests/control (autopkgtest)
const TESTS_CONTROL_FIELDS: &[&str] = &[
    "Tests",
    "Test-Command",
    "Restrictions",
    "Features",
    "Depends",
    "Tests-Directory",
    "Classes",
    "Architecture",
];

/// Field validator for debian/tests/control files
struct TestsControlFieldValidator;

impl FieldValidator for TestsControlFieldValidator {
    fn get_standard_field_name(&self, name: &str) -> Option<&'static str> {
        let lower = name.to_lowercase();
        TESTS_CONTROL_FIELDS
            .iter()
            .find(|f| f.to_lowercase() == lower)
            .copied()
    }
}

/// Generate semantic tokens for a debian/tests/control file
pub fn generate_semantic_tokens(source_text: &str) -> Vec<SemanticToken> {
    let parsed = deb822_lossless::Deb822::parse(source_text);
    let deb822 = parsed.tree();
    let validator = TestsControlFieldValidator;
    generate_tokens(&deb822, source_text, &validator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deb822::semantic::TokenType;

    #[test]
    fn test_known_fields() {
        let text = "Tests: my-test\nDepends: @\nRestrictions: needs-root\n";
        let tokens = generate_semantic_tokens(text);

        assert_eq!(tokens.len(), 6);

        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
        assert_eq!(tokens[0].length, 5); // "Tests"

        assert_eq!(tokens[2].token_type, TokenType::Field as u32);
        assert_eq!(tokens[2].length, 7); // "Depends"

        assert_eq!(tokens[4].token_type, TokenType::Field as u32);
        assert_eq!(tokens[4].length, 12); // "Restrictions"
    }

    #[test]
    fn test_unknown_field() {
        let text = "Tests: my-test\nX-Custom: value\n";
        let tokens = generate_semantic_tokens(text);

        assert_eq!(tokens[2].token_type, TokenType::UnknownField as u32);
        assert_eq!(tokens[2].length, 8); // "X-Custom"
    }

    #[test]
    fn test_case_insensitive() {
        let text = "tests: my-test\n";
        let tokens = generate_semantic_tokens(text);

        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
    }

    #[test]
    fn test_field_validator() {
        let validator = TestsControlFieldValidator;

        assert_eq!(validator.get_standard_field_name("Tests"), Some("Tests"));
        assert_eq!(validator.get_standard_field_name("tests"), Some("Tests"));
        assert_eq!(
            validator.get_standard_field_name("Test-Command"),
            Some("Test-Command")
        );
        assert_eq!(
            validator.get_standard_field_name("Restrictions"),
            Some("Restrictions")
        );
        assert_eq!(validator.get_standard_field_name("UnknownField"), None);
    }
}
