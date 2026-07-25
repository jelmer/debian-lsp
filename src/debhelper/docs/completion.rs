use std::path::Path;

use tower_lsp_server::ls_types::{CompletionItem, Position};

use crate::debhelper::completion;
use crate::debhelper::source::source_candidates;

/// Completions for a debian/docs file at the given cursor position.
pub fn get_completions(
    text: &str,
    position: Position,
    debian_dir: Option<&Path>,
) -> Vec<CompletionItem> {
    completion::get_completions(text, position, |_, prefix| match debian_dir {
        Some(dir) => source_candidates(dir, prefix),
        None => Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_scan::git_tree;
    use tower_lsp_server::ls_types::CompletionItemKind;

    fn labels(items: &[CompletionItem]) -> Vec<String> {
        items.iter().map(|i| i.label.clone()).collect()
    }

    #[test]
    fn completes_a_path_from_the_source_tree() {
        let dir = git_tree(&["debian/docs", "README", "TODO"], &[]);
        let debian = dir.path().join("debian");
        let items = get_completions("", Position::new(0, 0), Some(&debian));
        assert!(labels(&items).contains(&"README".to_string()));
        assert!(labels(&items).contains(&"TODO".to_string()));
    }

    #[test]
    fn filters_by_prefix() {
        let dir = git_tree(&["debian/docs", "doc/manual.txt", "src/main.rs"], &[]);
        let debian = dir.path().join("debian");
        let items = get_completions("doc/", Position::new(0, 4), Some(&debian));
        assert!(labels(&items).iter().all(|l| l.starts_with("doc/")));
    }

    #[test]
    fn nothing_without_a_debian_dir() {
        let items = get_completions("README", Position::new(0, 6), None);
        assert!(items.is_empty());
    }

    #[test]
    fn dollar_offers_substitution_vars() {
        let items = get_completions("$", Position::new(0, 1), None);
        assert!(items.iter().any(|i| i.label == "${DEB_HOST_MULTIARCH}"));
    }

    #[test]
    fn directories_end_with_a_slash() {
        let dir = git_tree(&["debian/docs", "doc/manual.txt"], &[]);
        let debian = dir.path().join("debian");
        let items = get_completions("doc", Position::new(0, 3), Some(&debian));
        let doc = items.iter().find(|i| i.label == "doc/").unwrap();
        assert_eq!(doc.kind, Some(CompletionItemKind::FOLDER));
    }

    #[test]
    fn no_completion_in_comment() {
        let items = get_completions("# README", Position::new(0, 8), None);
        assert!(items.is_empty());
    }
}
