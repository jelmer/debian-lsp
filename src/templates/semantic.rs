use tower_lsp_server::ls_types::SemanticToken;

use super::fields::get_standard_field_name;
use crate::deb822::semantic::{generate_tokens, FieldValidator};
use crate::position::Source;

struct TemplatesFieldValidator;

impl FieldValidator for TemplatesFieldValidator {
    fn get_standard_field_name(&self, name: &str) -> Option<&'static str> {
        for prefix in ["Description-", "Choices-", "Default-"] {
            if let Some(suffix) = name.strip_prefix(prefix) {
                if !suffix.is_empty() {
                    return Some(intern(name));
                }
            }
        }
        get_standard_field_name(name)
    }
}

/// Intern a string with `'static` lifetime in a process-wide cache so
/// `FieldValidator` can return it.
fn intern(name: &str) -> &'static str {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().expect("intern cache poisoned");
    if let Some(s) = guard.get(name) {
        return s;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    guard.insert(name.to_string(), leaked);
    leaked
}

/// Generate semantic tokens for a debconf templates file.
pub fn generate_semantic_tokens(text: &str, src: Source<'_>) -> Vec<SemanticToken> {
    let deb822 = deb822_lossless::Deb822::parse(text).tree();
    generate_tokens(&deb822, src, &TemplatesFieldValidator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deb822::semantic::TokenType;
    use crate::position::LineIndex;

    fn run(text: &str) -> Vec<SemanticToken> {
        let idx = LineIndex::new(text);
        generate_semantic_tokens(text, Source::new(text, &idx))
    }

    #[test]
    fn known_field_emits_field_token() {
        let tokens = run("Template: foo/bar\nType: select\n");
        assert!(!tokens.is_empty());
        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
    }

    #[test]
    fn unknown_field_emits_unknown_token() {
        let tokens = run("Template: foo/bar\nX-Custom: x\n");
        let kinds: Vec<u32> = tokens.iter().map(|t| t.token_type).collect();
        assert!(kinds.contains(&(TokenType::UnknownField as u32)));
    }

    #[test]
    fn localized_field_treated_as_known() {
        let tokens = run("Description: hi\nDescription-fr.UTF-8: bonjour\n");
        let field_tokens = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::Field as u32)
            .count();
        assert_eq!(field_tokens, 2);
    }

    #[test]
    fn translatable_master_fields_treated_as_known() {
        let tokens = run("_Description: hi\n__Choices: a, b\n_Default: a\n");
        let field_tokens = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::Field as u32)
            .count();
        assert_eq!(field_tokens, 3);
    }
}
