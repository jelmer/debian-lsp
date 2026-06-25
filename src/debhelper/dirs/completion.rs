use std::collections::HashSet;
use tower_lsp_server::ls_types::{CompletionItem, Position};

use crate::debhelper::completion::{dir_items, substitution_completions};

/// Get completions for a debian/dirs file at the given cursor position.
///
/// Every entry is a directory, so the whole line is the path prefix.
/// Directories already listed elsewhere in the file are excluded.
pub fn get_completions(text: &str, position: Position) -> Vec<CompletionItem> {
    let current_line = text.lines().nth(position.line as usize).unwrap_or("");

    let char_idx = (position.character as usize).min(current_line.len());
    let before_cursor = current_line.get(..char_idx).unwrap_or(current_line);

    if let Some(items) = substitution_completions(before_cursor) {
        return items;
    }

    // Collect already-listed directories to avoid suggesting duplicates.
    let existing: HashSet<&str> = text
        .lines()
        .enumerate()
        .filter(|(i, _)| *i != position.line as usize)
        .map(|(_, l)| l.trim().trim_start_matches('/'))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    dir_items(current_line.trim_start(), &existing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp_server::ls_types::CompletionItemKind;

    #[test]
    fn test_completion_empty_line() {
        let text = "\n";
        let items = get_completions(text, Position::new(0, 0));
        assert!(!items.is_empty());
        assert!(items.iter().any(|i| i.label == "usr/bin/"));
        assert!(items.iter().any(|i| i.label == "etc/"));
    }

    #[test]
    fn test_completion_filtered_by_prefix() {
        let text = "usr/\n";
        let items = get_completions(text, Position::new(0, 4));
        assert!(items.iter().all(|i| i.label.starts_with("usr/")));
        assert!(!items.iter().any(|i| i.label.starts_with("etc/")));
    }

    #[test]
    fn test_completion_excludes_existing() {
        let text = "usr/bin/\nusr/\n";
        let items = get_completions(text, Position::new(1, 4));
        assert!(!items.iter().any(|i| i.label == "usr/bin/"));
    }

    #[test]
    fn test_completion_leading_slash_stripped() {
        let text = "/usr/\n";
        let items = get_completions(text, Position::new(0, 5));
        assert!(items.iter().all(|i| i.label.starts_with("usr/")));
    }

    #[test]
    fn test_completion_items_are_folders() {
        let text = "\n";
        let items = get_completions(text, Position::new(0, 0));
        assert!(items
            .iter()
            .all(|i| i.kind == Some(CompletionItemKind::FOLDER)));
    }

    #[test]
    fn test_excludes_non_install_dirs() {
        let text = "\n";
        let items = get_completions(text, Position::new(0, 0));
        for bad in [
            "run/",
            "var/run/",
            "var/log/",
            "var/cache/",
            "srv/",
            "opt/",
            "lib/systemd/system/",
        ] {
            assert!(
                !items.iter().any(|i| i.label == bad),
                "should not suggest {bad}"
            );
        }
    }

    #[test]
    fn test_dollar_offers_substitution_vars() {
        let text = "usr/lib/$\n";
        let items = get_completions(text, Position::new(0, 9));
        assert!(items.iter().any(|i| i.label == "${DEB_HOST_MULTIARCH}"));
        let item = items
            .iter()
            .find(|i| i.label == "${DEB_HOST_MULTIARCH}")
            .unwrap();
        assert_eq!(item.insert_text, Some("{DEB_HOST_MULTIARCH}".to_string()));
        assert_eq!(item.kind, Some(CompletionItemKind::VARIABLE));
    }

    #[test]
    fn test_dollar_brace_offers_bare_names() {
        let text = "usr/lib/${\n";
        let items = get_completions(text, Position::new(0, 10));
        let item = items
            .iter()
            .find(|i| i.label == "${DEB_HOST_MULTIARCH}")
            .unwrap();
        assert_eq!(item.insert_text, Some("DEB_HOST_MULTIARCH".to_string()));
    }

    #[test]
    fn test_dollar_does_not_offer_directories() {
        let text = "usr/lib/$\n";
        let items = get_completions(text, Position::new(0, 9));
        assert!(!items.iter().any(|i| i.label == "usr/bin/"));
    }
}
