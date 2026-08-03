use std::path::Path;

use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use crate::debhelper::completion;
use crate::debhelper::source::source_candidates;

/// Completions for a debian/info file at the given cursor position.
pub fn get_completions(
    text: &str,
    position: Position,
    debian_dir: Option<&Path>,
) -> Vec<CompletionItem> {
    completion::get_completions(text, position, |_, prefix| match debian_dir {
        Some(dir) => info_pages(source_candidates(dir, prefix)),
        None => Vec::new(),
    })
}

/// Keep the directories to descend into and the files that are info pages.
fn info_pages(items: Vec<CompletionItem>) -> Vec<CompletionItem> {
    items
        .into_iter()
        .filter(|item| item.kind == Some(CompletionItemKind::FOLDER) || is_info_page(&item.label))
        .collect()
}

/// Whether `path` names an info page
fn is_info_page(path: &str) -> bool {
    let name = path.strip_suffix(".gz").unwrap_or(path);
    match name.rsplit_once(".info") {
        Some((_, rest)) => {
            rest.is_empty()
                || rest
                    .strip_prefix('-')
                    .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_scan::git_tree;

    fn labels(items: &[CompletionItem]) -> Vec<String> {
        items.iter().map(|i| i.label.clone()).collect()
    }

    #[test]
    fn completes_an_info_page_from_the_source_tree() {
        let dir = git_tree(&["debian/info", "foo.info", "README"], &[]);
        let debian = dir.path().join("debian");
        let items = get_completions("", Position::new(0, 0), Some(&debian));
        assert!(labels(&items).contains(&"foo.info".to_string()));
        assert!(!labels(&items).contains(&"README".to_string()));
    }

    #[test]
    fn offers_compressed_and_split_pages() {
        let dir = git_tree(&["debian/info", "foo.info.gz", "bar.info-1.gz"], &[]);
        let debian = dir.path().join("debian");
        let items = labels(&get_completions("", Position::new(0, 0), Some(&debian)));
        assert!(items.contains(&"foo.info.gz".to_string()));
        assert!(items.contains(&"bar.info-1.gz".to_string()));
    }

    #[test]
    fn a_lookalike_extension_is_not_an_info_page() {
        assert!(!is_info_page("doc/foo.infopage"));
        assert!(!is_info_page("doc/foo.info-x"));
        assert!(!is_info_page("doc/info"));
    }

    #[test]
    fn directories_are_kept_to_descend_into() {
        let dir = git_tree(&["debian/info", "doc/foo.info"], &[]);
        let debian = dir.path().join("debian");
        let items = get_completions("doc", Position::new(0, 3), Some(&debian));
        let doc = items.iter().find(|i| i.label == "doc/").unwrap();
        assert_eq!(doc.kind, Some(CompletionItemKind::FOLDER));
    }

    #[test]
    fn filters_by_prefix() {
        let dir = git_tree(&["debian/info", "doc/foo.info", "src/main.rs"], &[]);
        let debian = dir.path().join("debian");
        let items = get_completions("doc/", Position::new(0, 4), Some(&debian));
        assert!(labels(&items).iter().all(|l| l.starts_with("doc/")));
    }

    #[test]
    fn nothing_without_a_debian_dir() {
        let items = get_completions("doc/", Position::new(0, 4), None);
        assert!(items.is_empty());
    }

    #[test]
    fn dollar_offers_substitution_vars() {
        let items = get_completions("$", Position::new(0, 1), None);
        assert!(items.iter().any(|i| i.label == "${DEB_HOST_MULTIARCH}"));
    }

    #[test]
    fn no_completion_in_comment() {
        let items = get_completions("# doc/foo.info", Position::new(0, 14), None);
        assert!(items.is_empty());
    }
}
