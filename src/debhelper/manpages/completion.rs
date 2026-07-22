use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use crate::debhelper::completion;

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

/// Compression suffix a man page source may already carry.
const COMPRESSION_SUFFIXES: &[(&str, &str)] = &[("gz", "gzip-compressed man page")];

/// Completions for a debian/manpages file at the given cursor position.
pub fn get_completions(text: &str, position: Position) -> Vec<CompletionItem> {
    completion::get_completions(text, position, |_, prefix| extension_items(prefix))
}

/// Offer section extensions for the file name being typed.
fn extension_items(token: &str) -> Vec<CompletionItem> {
    let name = token.rsplit('/').next().unwrap_or(token);
    let Some((head, partial)) = name.rsplit_once('.') else {
        return Vec::new();
    };

    let prev = head.rsplit(['.', '/']).next().unwrap_or(head);
    let candidates: &[(&str, &str)] = if is_section(prev) {
        COMPRESSION_SUFFIXES
    } else {
        MAN_SECTIONS
    };

    candidates
        .iter()
        .filter(|(ext, _)| ext.starts_with(partial))
        .map(|&(ext, detail)| CompletionItem {
            label: ext.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some(detail.to_string()),
            ..Default::default()
        })
        .collect()
}

/// Whether `ext` is one of the recognized man section extensions.
fn is_section(ext: &str) -> bool {
    MAN_SECTIONS.iter().any(|&(s, _)| s == ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_completion_on_empty_line() {
        let items = get_completions("\n", Position::new(0, 0));
        assert!(items.is_empty());
    }

    #[test]
    fn no_completion_without_dot() {
        let items = get_completions("foo\n", Position::new(0, 3));
        assert!(items.is_empty());
    }

    #[test]
    fn offers_sections_after_dot() {
        let items = get_completions("foo.\n", Position::new(0, 4));
        assert!(items.iter().any(|i| i.label == "1"));
        assert!(items.iter().any(|i| i.label == "8"));
        assert!(items.iter().any(|i| i.label == "3pm"));
        assert!(items
            .iter()
            .all(|i| i.kind == Some(CompletionItemKind::VALUE)));
    }

    #[test]
    fn filters_by_partial_section() {
        let items = get_completions("foo.3\n", Position::new(0, 5));
        assert!(items.iter().all(|i| i.label.starts_with('3')));
        assert!(items.iter().any(|i| i.label == "3pm"));
        assert!(!items.iter().any(|i| i.label == "1"));
    }

    #[test]
    fn offers_compression_after_section() {
        let items = get_completions("foo.1.\n", Position::new(0, 6));
        assert!(items.iter().any(|i| i.label == "gz"));
        assert!(!items.iter().any(|i| i.label == "1"));
    }

    #[test]
    fn directory_prefix_ignored() {
        let items = get_completions("usr/share/man/man1/foo.\n", Position::new(0, 23));
        assert!(items.iter().any(|i| i.label == "1"));
    }

    #[test]
    fn glob_name_offers_sections() {
        let items = get_completions("debian/tmp/usr/bin/*.\n", Position::new(0, 21));
        assert!(items.iter().any(|i| i.label == "1"));
    }

    #[test]
    fn no_completion_at_new_token() {
        let items = get_completions("foo.1 \n", Position::new(0, 6));
        assert!(items.is_empty());
    }

    #[test]
    fn no_completion_in_comment() {
        let items = get_completions("# install foo.1\n", Position::new(0, 15));
        assert!(items.is_empty());
    }

    #[test]
    fn dollar_offers_substitution_vars() {
        let items = get_completions("foo.$\n", Position::new(0, 5));
        let item = items
            .iter()
            .find(|i| i.label == "${DEB_HOST_MULTIARCH}")
            .unwrap();
        assert_eq!(item.insert_text, Some("{DEB_HOST_MULTIARCH}".to_string()));
    }

    #[test]
    fn dollar_brace_offers_bare_names() {
        let items = get_completions("foo.${\n", Position::new(0, 6));
        let item = items.iter().find(|i| i.label == "${Space}").unwrap();
        assert_eq!(item.insert_text, Some("Space".to_string()));
    }
}
