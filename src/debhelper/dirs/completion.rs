use tower_lsp_server::ls_types::{CompletionItem, Position};

use crate::debhelper::completion::{common_completions, dir_items, other_entries};
use crate::debhelper::parser::CursorContext;

/// Completions for a debian/dirs file at the given cursor position.
pub fn get_completions(text: &str, position: Position) -> Vec<CompletionItem> {
    let line = text.lines().nth(position.line as usize).unwrap_or("");
    let offset = (position.character as usize).min(line.len());
    let cx = CursorContext::at(line, offset);

    // Comments and substitutions are handled the same for every debhelper file.
    if let Some(items) = common_completions(&cx) {
        return items;
    }

    // A dirs entry is a single directory path. Complete it against the common
    // install locations, dropping any already listed elsewhere in the file.
    dir_items(cx.prefix, &other_entries(text, position.line as usize))
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

    #[test]
    fn test_comment_offers_nothing() {
        let text = "# usr/lib/\n";
        let items = get_completions(text, Position::new(0, 10));
        assert!(items.is_empty());
    }
}
