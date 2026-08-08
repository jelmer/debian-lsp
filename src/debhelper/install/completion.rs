use std::collections::HashSet;
use std::path::Path;

use tower_lsp_server::ls_types::{CompletionItem, Position};

use crate::debhelper::completion::{self, dir_items};
use crate::debhelper::source::source_candidates;

/// Completions for a debian/install file at the given cursor position.
pub fn get_completions(
    text: &str,
    position: Position,
    debian_dir: Option<&Path>,
) -> Vec<CompletionItem> {
    completion::get_completions(text, position, |index, prefix| {
        let mut items = match debian_dir {
            Some(dir) => source_candidates(dir, prefix),
            None => Vec::new(),
        };
        if index > 0 {
            items.extend(dir_items(prefix, &HashSet::new()));
        }
        items
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_scan::git_tree;
    use tower_lsp_server::ls_types::CompletionItemKind;

    #[test]
    fn no_source_completion_without_a_debian_dir() {
        let items = get_completions("my-pr\n", Position::new(0, 5), None);
        assert!(items.is_empty());
    }

    #[test]
    fn source_offers_files_from_the_package() {
        let dir = git_tree(&["debian/install", "usr/bin/prog", "README"], &[]);
        let debian = dir.path().join("debian");
        let items = get_completions("usr/\n", Position::new(0, 4), Some(&debian));
        assert!(items.iter().any(|i| i.label == "usr/bin/"));
        assert!(!items.iter().any(|i| i.label == "README"));
    }

    #[test]
    fn no_completion_on_an_empty_line() {
        assert!(get_completions("\n", Position::new(0, 0), None).is_empty());
    }

    #[test]
    fn destination_offers_dirs_after_the_space() {
        let items = get_completions("my-prog \n", Position::new(0, 8), None);
        assert!(items.iter().any(|i| i.label == "usr/bin/"));
        assert!(items.iter().any(|i| i.label == "etc/"));
        assert!(items
            .iter()
            .all(|i| i.kind == Some(CompletionItemKind::FOLDER)));
    }

    #[test]
    fn destination_filtered_by_prefix() {
        let items = get_completions("my-prog usr/\n", Position::new(0, 12), None);
        assert!(items.iter().all(|i| i.label.starts_with("usr/")));
        assert!(!items.iter().any(|i| i.label.starts_with("etc/")));
    }

    #[test]
    fn destination_leading_slash_stripped_for_matching() {
        let items = get_completions("my-prog /usr/\n", Position::new(0, 13), None);
        assert!(items.iter().all(|i| i.label.starts_with("usr/")));
    }

    #[test]
    fn a_later_token_still_completes_as_a_source() {
        let dir = git_tree(&["debian/install", "usr/bin/prog"], &[]);
        let debian = dir.path().join("debian");
        let items = get_completions("a b usr/\n", Position::new(0, 8), Some(&debian));
        assert!(items.iter().any(|i| i.label == "usr/bin/"));
    }

    #[test]
    fn dollar_offers_substitution_vars() {
        let items = get_completions("my-prog usr/lib/$\n", Position::new(0, 17), None);
        let item = items
            .iter()
            .find(|i| i.label == "${DEB_HOST_MULTIARCH}")
            .unwrap();
        assert_eq!(item.insert_text, Some("{DEB_HOST_MULTIARCH}".to_string()));
    }

    #[test]
    fn dollar_brace_offers_bare_names() {
        let items = get_completions("my-prog usr/lib/${\n", Position::new(0, 18), None);
        let item = items.iter().find(|i| i.label == "${Space}").unwrap();
        assert_eq!(item.insert_text, Some("Space".to_string()));
    }

    #[test]
    fn dollar_on_the_source_token_still_offers_vars() {
        let items = get_completions("my$\n", Position::new(0, 3), None);
        assert!(items.iter().any(|i| i.label == "${Space}"));
    }

    #[test]
    fn no_completion_in_a_comment() {
        let items = get_completions("# install my-prog into usr/\n", Position::new(0, 27), None);
        assert!(items.is_empty());
    }
}
