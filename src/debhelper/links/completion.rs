use std::path::Path;

use tower_lsp_server::ls_types::{CompletionItem, Position};

use crate::debhelper::completion;
use crate::debhelper::source::dir_candidates;

/// Completions for a debian/links file at the given cursor position.
pub fn get_completions(
    text: &str,
    position: Position,
    package_dir: Option<&Path>,
) -> Vec<CompletionItem> {
    completion::get_completions(text, position, |_, prefix| match package_dir {
        Some(dir) => dir_candidates(dir, prefix),
        None => Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp_server::ls_types::CompletionItemKind;

    fn staging(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for rel in files {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
        }
        dir
    }

    fn labels(items: &[CompletionItem]) -> Vec<String> {
        items.iter().map(|i| i.label.clone()).collect()
    }

    #[test]
    fn completes_the_target_token() {
        let pkg = staging(&["usr/bin/prog"]);
        let items = get_completions("usr/\n", Position::new(0, 4), Some(pkg.path()));
        assert!(labels(&items).contains(&"usr/bin/".to_string()));
    }

    #[test]
    fn completes_the_link_token_from_the_package_dir() {
        // Two distinct files: the target is at usr/bin/prog, the link is
        // meant to live at usr/local/foo. Whichever token the cursor is on,
        // completions must come from the same package staging directory --
        // the test asserts both tokens see the same layout.
        let pkg = staging(&["usr/bin/prog", "usr/local/foo"]);
        let items = get_completions(
            "usr/bin/prog usr/local/\n",
            Position::new(0, 23),
            Some(pkg.path()),
        );
        assert!(labels(&items).contains(&"usr/local/foo".to_string()));
    }

    #[test]
    fn a_later_token_still_completes() {
        let pkg = staging(&["usr/bin/prog"]);
        let items = get_completions("a b usr/\n", Position::new(0, 8), Some(pkg.path()));
        assert!(labels(&items).iter().any(|l| l.starts_with("usr/")));
    }

    #[test]
    fn a_third_token_still_completes() {
        // dh_link takes source/destination pairs; a stray third token is a
        // user error, but the completer should still offer sensible items
        // rather than silently give up.
        let pkg = staging(&["usr/bin/prog"]);
        let items = get_completions("a b usr/\n", Position::new(0, 8), Some(pkg.path()));
        assert!(labels(&items).contains(&"usr/bin/".to_string()));
    }

    #[test]
    fn directories_end_with_a_slash() {
        let pkg = staging(&["usr/bin/prog"]);
        let items = get_completions("usr", Position::new(0, 3), Some(pkg.path()));
        let usr = items.iter().find(|i| i.label == "usr/").unwrap();
        assert_eq!(usr.kind, Some(CompletionItemKind::FOLDER));
    }

    #[test]
    fn nothing_without_a_package_dir() {
        let items = get_completions("usr/\n", Position::new(0, 4), None);
        assert!(items.is_empty());
    }

    #[test]
    fn dollar_offers_substitution_vars() {
        let items = get_completions("usr/lib/$\n", Position::new(0, 9), None);
        assert!(items.iter().any(|i| i.label == "${DEB_HOST_MULTIARCH}"));
    }

    #[test]
    fn no_completion_at_end_of_comment() {
        let items = get_completions("# usr/share/foo\n", Position::new(0, 15), None);
        assert!(items.is_empty());
    }

    #[test]
    fn no_completion_mid_comment() {
        let items = get_completions("# usr/share/foo\n", Position::new(0, 8), None);
        assert!(items.is_empty());
    }
}
