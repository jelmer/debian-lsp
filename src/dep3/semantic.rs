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

/// Generate semantic tokens for the DEP-3 header portion of a patch
/// file. Returns an empty vector if the header is empty or fails to
/// parse as deb822. Tokens are emitted only for the header — diff
/// lines are left untouched for diff-lsp.
pub fn generate_semantic_tokens(source_text: &str) -> Vec<SemanticToken> {
    let header_end = dep3::lossless::header_end(source_text);
    if header_end == 0 {
        return Vec::new();
    }
    let header_text = &source_text[..header_end];
    let parsed = deb822_lossless::Deb822::parse(header_text);
    let deb822 = parsed.tree();
    // The shared helper expects the source text to match the parsed
    // tree's offsets. We pass the header substring (not the full file)
    // and the resulting tokens' line/column deltas line up with the
    // editor's view because the header substring shares a 0-offset
    // with the buffer.
    let validator = Dep3FieldValidator;
    generate_tokens(&deb822, source_text, &validator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deb822::semantic::TokenType;

    #[test]
    fn known_field_emits_field_token() {
        let text = "Author: alice\nDescription: bla\n";
        let tokens = generate_semantic_tokens(text);
        assert!(!tokens.is_empty());
        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
        assert_eq!(tokens[0].length, 6);
    }

    #[test]
    fn unknown_field_emits_unknown_token() {
        let text = "Author: alice\nX-Custom: x\n";
        let tokens = generate_semantic_tokens(text);
        let kinds: Vec<u32> = tokens.iter().map(|t| t.token_type).collect();
        assert!(kinds.contains(&(TokenType::UnknownField as u32)));
    }

    #[test]
    fn vendor_bug_field_treated_as_known() {
        let text = "Author: alice\nBug-Debian: https://bugs.debian.org/123\n";
        let tokens = generate_semantic_tokens(text);
        let field_tokens: Vec<&SemanticToken> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::Field as u32)
            .collect();
        assert_eq!(field_tokens.len(), 2);
    }

    #[test]
    fn diff_body_does_not_emit_tokens() {
        let text = "Author: alice\n---\n+++ b/foo\n+@@ -1 +1 @@\n";
        let tokens = generate_semantic_tokens(text);
        let field_tokens: Vec<&SemanticToken> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::Field as u32)
            .collect();
        assert_eq!(field_tokens.len(), 1);
    }
}
