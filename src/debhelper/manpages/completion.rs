use std::path::Path;

use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use crate::debhelper::completion;
use crate::debhelper::source::source_candidates;

/// Man page section extensions, with what each section holds.
pub const MAN_SECTIONS: &[(&str, &str)] = &[
    ("1", "Executable programs and shell commands"),
    ("2", "System calls"),
    ("3", "Library functions"),
    ("4", "Special files (usually in /dev)"),
    ("5", "File formats and conventions"),
    ("6", "Games"),
    ("7", "Miscellaneous"),
    ("8", "System administration commands"),
    ("9", "Kernel routines"),
    ("0p", "POSIX header"),
    ("1p", "POSIX command"),
    ("3p", "POSIX library function"),
    ("3pm", "Perl module"),
    ("3perl", "Perl core"),
    ("1ssl", "OpenSSL command"),
    ("3ssl", "OpenSSL library function"),
    ("5ssl", "OpenSSL file format"),
    ("7ssl", "OpenSSL miscellaneous"),
    ("3am", "GNU Awk extension"),
    ("n", "Tcl/Tk command"),
];

/// Completions for a debian/manpages file at the given cursor position.
pub fn get_completions(
    text: &str,
    position: Position,
    debian_dir: Option<&Path>,
) -> Vec<CompletionItem> {
    completion::get_completions(text, position, |_, prefix| match debian_dir {
        Some(dir) => man_pages(source_candidates(dir, prefix)),
        None => Vec::new(),
    })
}

/// Keep the directories to descend into and the files whose extension is a man
/// section, described by what that section holds.
fn man_pages(items: Vec<CompletionItem>) -> Vec<CompletionItem> {
    items
        .into_iter()
        .filter_map(|mut item| {
            if item.kind == Some(CompletionItemKind::FOLDER) {
                return Some(item);
            }
            item.detail = Some(section_detail(&item.label)?.to_string());
            Some(item)
        })
        .collect()
}

/// What the section of the man page at `path` holds, if it is one.
fn section_detail(path: &str) -> Option<&'static str> {
    let name = path.strip_suffix(".gz").unwrap_or(path);
    let (_, extension) = name.rsplit_once('.')?;
    MAN_SECTIONS
        .iter()
        .find(|&&(section, _)| section == extension)
        .map(|&(_, detail)| detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_scan::git_tree;

    fn labels(items: &[CompletionItem]) -> Vec<String> {
        items.iter().map(|i| i.label.clone()).collect()
    }

    #[test]
    fn completes_a_man_page_from_the_source_tree() {
        let dir = git_tree(&["debian/manpages", "foo.1", "README"], &[]);
        let debian = dir.path().join("debian");
        let items = labels(&get_completions("", Position::new(0, 0), Some(&debian)));
        assert!(items.contains(&"foo.1".to_string()));
        assert!(!items.contains(&"README".to_string()));
    }

    #[test]
    fn offers_compressed_and_lettered_sections() {
        let dir = git_tree(&["debian/manpages", "foo.8.gz", "Bar.3pm", "tk.n"], &[]);
        let debian = dir.path().join("debian");
        let items = labels(&get_completions("", Position::new(0, 0), Some(&debian)));
        assert!(items.contains(&"foo.8.gz".to_string()));
        assert!(items.contains(&"Bar.3pm".to_string()));
        assert!(items.contains(&"tk.n".to_string()));
    }

    #[test]
    fn a_page_says_what_its_section_holds() {
        let dir = git_tree(&["debian/manpages", "foo.1"], &[]);
        let debian = dir.path().join("debian");
        let items = get_completions("", Position::new(0, 0), Some(&debian));
        let page = items.iter().find(|i| i.label == "foo.1").unwrap();
        assert_eq!(
            page.detail,
            Some("Executable programs and shell commands".to_string())
        );
    }

    #[test]
    fn an_unknown_extension_is_not_a_man_page() {
        assert!(section_detail("doc/foo.txt").is_none());
        assert!(section_detail("doc/foo.1x").is_none());
        assert!(section_detail("doc/foo").is_none());
    }

    #[test]
    fn directories_are_kept_to_descend_into() {
        let dir = git_tree(&["debian/manpages", "doc/foo.1"], &[]);
        let debian = dir.path().join("debian");
        let items = get_completions("doc", Position::new(0, 3), Some(&debian));
        let doc = items.iter().find(|i| i.label == "doc/").unwrap();
        assert_eq!(doc.kind, Some(CompletionItemKind::FOLDER));
    }

    #[test]
    fn filters_by_prefix() {
        let dir = git_tree(&["debian/manpages", "doc/foo.1", "src/bar.1"], &[]);
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
    fn no_completion_in_comment() {
        let items = get_completions("# doc/foo.1", Position::new(0, 11), None);
        assert!(items.is_empty());
    }

    #[test]
    fn dollar_offers_substitution_vars() {
        let items = get_completions("$", Position::new(0, 1), None);
        let item = items
            .iter()
            .find(|i| i.label == "${DEB_HOST_MULTIARCH}")
            .unwrap();
        assert_eq!(item.insert_text, Some("{DEB_HOST_MULTIARCH}".to_string()));
    }

    #[test]
    fn dollar_brace_offers_bare_names() {
        let items = get_completions("${", Position::new(0, 2), None);
        let item = items.iter().find(|i| i.label == "${Space}").unwrap();
        assert_eq!(item.insert_text, Some("Space".to_string()));
    }
}
