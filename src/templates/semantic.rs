use tower_lsp_server::ls_types::SemanticToken;

use super::fields::get_standard_field_name;
use crate::deb822::semantic::{generate_tokens, FieldValidator};
use crate::position::Source;

/// Field validator for debconf template files.
///
/// Accepts the fixed set from [`super::fields::TEMPLATES_FIELDS`], plus
/// localized `Description-<locale>` / `Choices-<locale>` / `Default-<locale>`
/// forms (the locale suffix is checked to look like a POSIX locale name
/// rather than accepting any non-empty suffix).
///
/// Note: this is intentionally more permissive than [`super::hover::get_hover`],
/// which only offers hover text for the master field names. Localized
/// variants are highlighted as known so an editor doesn't paint them red,
/// but they have no hover documentation of their own. Same split as dep3.
struct TemplatesFieldValidator;

impl FieldValidator for TemplatesFieldValidator {
    fn get_standard_field_name(&self, name: &str) -> Option<&'static str> {
        for prefix in ["Description-", "Choices-", "Default-"] {
            if let Some(suffix) = name.strip_prefix(prefix) {
                if is_locale_suffix(suffix) {
                    return Some(intern(name));
                }
            }
        }
        get_standard_field_name(name)
    }
}

/// Whether `suffix` looks like a POSIX locale name.
///
/// po-debconf writes localized fields as `Description-<locale>`, where
/// `<locale>` is a POSIX locale identifier such as `fr`, `fr_CA`,
/// `fr.UTF-8`, or `zh_CN.UTF-8@variant`. We accept the character set that
/// covers these forms and reject empty or garbage suffixes so noise like
/// `Description- ` or `Description-!!` is highlighted as unknown.
fn is_locale_suffix(suffix: &str) -> bool {
    !suffix.is_empty()
        && suffix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '@'))
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
pub fn generate_semantic_tokens(
    deb822: &deb822_lossless::Deb822,
    src: Source<'_>,
) -> Vec<SemanticToken> {
    generate_tokens(deb822, src, &TemplatesFieldValidator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deb822::semantic::TokenType;
    use crate::position::LineIndex;

    fn run(text: &str) -> Vec<SemanticToken> {
        let deb822 = deb822_lossless::Deb822::parse(text).tree();
        let idx = LineIndex::new(text);
        generate_semantic_tokens(&deb822, Source::new(text, &idx))
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

    #[test]
    fn locale_suffix_accepts_common_forms() {
        assert!(is_locale_suffix("fr"));
        assert!(is_locale_suffix("fr_CA"));
        assert!(is_locale_suffix("fr.UTF-8"));
        assert!(is_locale_suffix("zh_CN.UTF-8"));
        assert!(is_locale_suffix("de@euro"));
    }

    #[test]
    fn locale_suffix_rejects_garbage() {
        assert!(!is_locale_suffix(""));
        assert!(!is_locale_suffix(" "));
        assert!(!is_locale_suffix("!!"));
        assert!(!is_locale_suffix("fr FR"));
    }
}
