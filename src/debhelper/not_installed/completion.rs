use std::path::Path;

use tower_lsp_server::ls_types::{CompletionItem, Position};

use crate::debhelper::completion;
use crate::debhelper::source::staged_candidates;

/// Completions for a debian/not-installed file at the given cursor position.
pub fn get_completions(
    text: &str,
    position: Position,
    debian_dir: Option<&Path>,
) -> Vec<CompletionItem> {
    completion::get_completions(text, position, |_, prefix| match debian_dir {
        Some(dir) => staged_candidates(dir, prefix),
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
    fn completes_a_path_from_debian_tmp() {
        let dir = git_tree(
            &["debian/not-installed", "debian/tmp/usr/bin/extra", "README"],
            &[],
        );
        let debian = dir.path().join("debian");
        let items = get_completions("", Position::new(0, 0), Some(&debian));
        assert!(labels(&items).contains(&"usr/".to_string()));
        assert!(!labels(&items).contains(&"README".to_string()));
    }

    #[test]
    fn filters_by_prefix() {
        let dir = git_tree(
            &[
                "debian/not-installed",
                "debian/tmp/usr/bin/extra",
                "debian/tmp/etc/conf",
            ],
            &[],
        );
        let debian = dir.path().join("debian");
        let items = get_completions("usr/", Position::new(0, 4), Some(&debian));
        assert!(labels(&items).iter().all(|l| l.starts_with("usr/")));
        assert!(!labels(&items).iter().any(|l| l.starts_with("etc")));
    }

    #[test]
    fn nothing_without_a_debian_dir() {
        let items = get_completions("usr/", Position::new(0, 4), None);
        assert!(items.is_empty());
    }

    #[test]
    fn dollar_offers_substitution_vars() {
        let items = get_completions("$", Position::new(0, 1), None);
        assert!(items.iter().any(|i| i.label == "${DEB_HOST_MULTIARCH}"));
    }

    #[test]
    fn directories_end_with_a_slash() {
        let dir = git_tree(&["debian/not-installed", "debian/tmp/usr/bin/extra"], &[]);
        let debian = dir.path().join("debian");
        let items = get_completions("usr", Position::new(0, 3), Some(&debian));
        let usr = items.iter().find(|i| i.label == "usr/").unwrap();
        assert_eq!(usr.kind, Some(CompletionItemKind::FOLDER));
    }

    #[test]
    fn no_completion_in_comment() {
        let items = get_completions("# usr/", Position::new(0, 6), None);
        assert!(items.is_empty());
    }
}
