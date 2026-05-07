//! Semantic token generation for DEP-3 patch headers.
//!
//! Tokens cover only the header portion of the file — anything past
//! the first `---` / `diff ` / `Index:` line is the unified diff and
//! gets left to diff-lsp.

use tower_lsp_server::ls_types::SemanticToken;

use super::get_standard_field_name;
use crate::deb822::semantic::{generate_tokens, FieldValidator};

struct Dep3FieldValidator;

impl FieldValidator for Dep3FieldValidator {
    fn get_standard_field_name(&self, name: &str) -> Option<&'static str> {
        // `Bug-<Vendor>` headers are valid DEP-3 (the spec lists
        // `Bug-Debian` as the canonical example) but vendor-specific so
        // we don't enumerate them. Treat any non-empty `Bug-…` as known.
        if let Some(stripped) = name.strip_prefix("Bug-") {
            if !stripped.is_empty() {
                return Some(intern(name));
            }
        }
        get_standard_field_name(name)
    }
}

/// Intern a string with `'static` lifetime in a process-wide cache so
/// `FieldValidator` can return it. Vendor names (`Debian`, `Ubuntu`, …)
/// recur across an editing session, so the leak is bounded by the
/// distinct set of vendors the user touches.
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

/// Generate semantic tokens for a DEP-3 header. `header` is the
/// parsed deb822 of the header portion only; `source_text` is the
/// whole patch buffer, used by the underlying token generator for
/// position math. Tokens are emitted only for the header — the diff
/// body is left for diff-lsp.
pub fn generate_semantic_tokens(
    header: &deb822_lossless::Deb822,
    source_text: &str,
) -> Vec<SemanticToken> {
    let validator = Dep3FieldValidator;
    generate_tokens(header, source_text, &validator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deb822::semantic::TokenType;

    fn run(text: &str) -> Vec<SemanticToken> {
        let header_end = dep3::lossless::header_end(text);
        let parsed = deb822_lossless::Deb822::parse(&text[..header_end]);
        generate_semantic_tokens(&parsed.tree(), text)
    }

    #[test]
    fn known_field_emits_field_token() {
        let tokens = run("Author: alice\nDescription: bla\n");
        assert!(!tokens.is_empty());
        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
        assert_eq!(tokens[0].length, 6);
    }

    #[test]
    fn unknown_field_emits_unknown_token() {
        let tokens = run("Author: alice\nX-Custom: x\n");
        let kinds: Vec<u32> = tokens.iter().map(|t| t.token_type).collect();
        assert!(kinds.contains(&(TokenType::UnknownField as u32)));
    }

    #[test]
    fn vendor_bug_field_treated_as_known() {
        let tokens = run("Author: alice\nBug-Debian: https://bugs.debian.org/123\n");
        let field_tokens: Vec<&SemanticToken> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::Field as u32)
            .collect();
        assert_eq!(field_tokens.len(), 2);
    }

    #[test]
    fn diff_body_does_not_emit_tokens() {
        let tokens = run("Author: alice\n---\n+++ b/foo\n+@@ -1 +1 @@\n");
        let field_tokens: Vec<&SemanticToken> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::Field as u32)
            .collect();
        assert_eq!(field_tokens.len(), 1);
    }
}
