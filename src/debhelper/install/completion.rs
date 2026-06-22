use std::collections::HashSet;
use tower_lsp_server::ls_types::{CompletionItem, Position};

use crate::debhelper::completion::{dir_items, substitution_completions};

/// Get completions for a debian/install file at the given cursor position.
///
/// An entry is `<source> <destination>`. Only the destination token gets
/// directory completions; the source is a path into the source tree we
/// can't usefully suggest.
pub fn get_completions(text: &str, position: Position) -> Vec<CompletionItem> {
    let line = text.lines().nth(position.line as usize).unwrap_or("");
    let char_idx = (position.character as usize).min(line.len());
    let before = line.get(..char_idx).unwrap_or(line);

    // Substitution variables after `$` / `${`, anywhere on the line.
    if let Some(items) = substitution_completions(before) {
        return items;
    }

    // Don't complete inside a comment line.
    if before.trim_start().starts_with('#') {
        return Vec::new();
    }

    // Determine which token the cursor sits on. Token 0 is the source,
    // token 1 is the destination. ${Space} carries no literal space, so
    // splitting on whitespace handles spaces-in-filenames entries fine.
    let tokens_before: Vec<&str> = before.split_whitespace().collect();
    let token_index = if before.is_empty() {
        0
    } else if before.ends_with(char::is_whitespace) {
        tokens_before.len()
    } else {
        tokens_before.len().saturating_sub(1)
    };

    // Only the destination (token 1) gets directory completions.
    if token_index != 1 {
        return Vec::new();
    }

    // Prefix-filter the directory list by the partial destination token.
    let dest_prefix = if before.ends_with(char::is_whitespace) {
        ""
    } else {
        tokens_before.last().copied().unwrap_or("")
    };

    dir_items(dest_prefix, &HashSet::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp_server::ls_types::CompletionItemKind;

    #[test]
    fn test_no_completion_on_source_token() {
        let text = "my-pr\n";
        let items = get_completions(text, Position::new(0, 5));
        assert!(items.is_empty());
    }

    #[test]
    fn test_no_completion_on_empty_line() {
        let items = get_completions("\n", Position::new(0, 0));
        assert!(items.is_empty());
    }

    #[test]
    fn test_destination_offers_dirs_after_space() {
        let text = "my-prog \n";
        let items = get_completions(text, Position::new(0, 8));
        assert!(items.iter().any(|i| i.label == "usr/bin/"));
        assert!(items.iter().any(|i| i.label == "etc/"));
        assert!(items
            .iter()
            .all(|i| i.kind == Some(CompletionItemKind::FOLDER)));
    }

    #[test]
    fn test_destination_filtered_by_prefix() {
        let text = "my-prog usr/\n";
        let items = get_completions(text, Position::new(0, 12));
        assert!(items.iter().all(|i| i.label.starts_with("usr/")));
        assert!(!items.iter().any(|i| i.label.starts_with("etc/")));
    }

    #[test]
    fn test_destination_leading_slash_stripped_for_matching() {
        let text = "my-prog /usr/\n";
        let items = get_completions(text, Position::new(0, 13));
        assert!(items.iter().all(|i| i.label.starts_with("usr/")));
    }

    #[test]
    fn test_no_completion_on_third_token() {
        let text = "my-prog usr/bin \n";
        let items = get_completions(text, Position::new(0, 16));
        assert!(items.is_empty());
    }

    #[test]
    fn test_dollar_offers_substitution_vars() {
        let text = "my-prog usr/lib/$\n";
        let items = get_completions(text, Position::new(0, 17));
        assert!(items.iter().any(|i| i.label == "${DEB_HOST_MULTIARCH}"));
        let item = items
            .iter()
            .find(|i| i.label == "${DEB_HOST_MULTIARCH}")
            .unwrap();
        assert_eq!(item.insert_text, Some("{DEB_HOST_MULTIARCH}".to_string()));
    }

    #[test]
    fn test_dollar_brace_offers_bare_names() {
        let text = "my-prog usr/lib/${\n";
        let items = get_completions(text, Position::new(0, 18));
        let item = items.iter().find(|i| i.label == "${Space}").unwrap();
        assert_eq!(item.insert_text, Some("Space".to_string()));
    }

    #[test]
    fn test_dollar_on_source_token_still_offers_vars() {
        let text = "my$\n";
        let items = get_completions(text, Position::new(0, 3));
        assert!(items.iter().any(|i| i.label == "${Space}"));
    }

    #[test]
    fn test_no_completion_in_comment() {
        let text = "# install my-prog into usr/\n";
        let items = get_completions(text, Position::new(0, 27));
        assert!(items.is_empty());
    }
}
