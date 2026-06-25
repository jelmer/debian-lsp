//! Completion building blocks shared by line-based debhelper config files.
//!
//! The pieces that every debhelper helper needs are here: expanding the
//! `$`/`${` substitution variables and offering install-directory paths.
//! How a file decides *where* on the line a directory belongs (the whole
//! line for dirs, only the destination token for install) stays in each
//! module's own `get_completions`.

use std::collections::HashSet;
use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind};

/// Common installation directory prefixes offered as completions.
pub const COMMON_DIRS: &[&str] = &[
    "etc/",
    "etc/default/",
    "etc/init.d/",
    "etc/systemd/system/",
    "usr/bin/",
    "usr/sbin/",
    "usr/lib/",
    "usr/lib/systemd/system/",
    "usr/share/",
    "usr/share/doc/",
    "usr/share/man/",
    "usr/share/man/man1/",
    "usr/share/man/man5/",
    "usr/share/man/man8/",
    "usr/share/applications/",
    "usr/share/icons/",
    "usr/share/locale/",
];

/// Substitution variables available in debhelper config files.
pub const SUBSTITUTION_VARS: &[(&str, &str)] = &[
    ("DEB_HOST_ARCH", "dpkg-architecture host architecture"),
    (
        "DEB_HOST_MULTIARCH",
        "dpkg-architecture host multiarch tuple",
    ),
    ("DEB_HOST_ARCH_OS", "dpkg-architecture host OS"),
    ("DEB_BUILD_ARCH", "dpkg-architecture build architecture"),
    (
        "DEB_BUILD_MULTIARCH",
        "dpkg-architecture build multiarch tuple",
    ),
    (
        "DEB_TARGET_ARCH",
        "dpkg-architecture target architecture (cross builds)",
    ),
    ("Dollar", "A literal '$' character"),
    ("Newline", "A literal newline character"),
    ("Space", "A literal space character"),
    ("Tab", "A literal tab character"),
];

/// Build completion items for the debhelper substitution variables.
fn substitution_var_items(needs_braces: bool) -> Vec<CompletionItem> {
    SUBSTITUTION_VARS
        .iter()
        .map(|&(name, detail)| {
            let insert_text = if needs_braces {
                format!("{{{name}}}")
            } else {
                name.to_string()
            };
            CompletionItem {
                label: format!("${{{name}}}"),
                insert_text: Some(insert_text),
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some(detail.to_string()),
                ..Default::default()
            }
        })
        .collect()
}

/// If the text just before the cursor opens a substitution variable, return
/// the matching variable completions.
///
/// debhelper expands `${VAR}` (and bare `$VAR`) in any line-based config
/// file, so this handling is identical everywhere. Returns `None` when the
/// cursor is not right after a `$` or `${`, leaving the caller to decide
/// what else to offer.
pub fn substitution_completions(before_cursor: &str) -> Option<Vec<CompletionItem>> {
    if before_cursor.ends_with("${") {
        Some(substitution_var_items(false))
    } else if before_cursor.ends_with('$') {
        Some(substitution_var_items(true))
    } else {
        None
    }
}

/// Build directory completions from [`COMMON_DIRS`], filtered by `prefix`
/// (a leading slash is ignored) and excluding anything already in `taken`.
///
/// Pass an empty set for `taken` when there is nothing to exclude.
pub fn dir_items(prefix: &str, taken: &HashSet<&str>) -> Vec<CompletionItem> {
    let prefix = prefix.trim_start_matches('/');
    COMMON_DIRS
        .iter()
        .filter(|&&dir| dir.starts_with(prefix) && !taken.contains(dir))
        .map(|&dir| CompletionItem {
            label: dir.to_string(),
            kind: Some(CompletionItemKind::FOLDER),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dollar_offers_braced_names() {
        let items = substitution_completions("usr/lib/$").unwrap();
        let item = items
            .iter()
            .find(|i| i.label == "${DEB_HOST_MULTIARCH}")
            .unwrap();
        assert_eq!(item.insert_text, Some("{DEB_HOST_MULTIARCH}".to_string()));
    }

    #[test]
    fn test_dollar_brace_offers_bare_names() {
        let items = substitution_completions("usr/lib/${").unwrap();
        let item = items.iter().find(|i| i.label == "${Space}").unwrap();
        assert_eq!(item.insert_text, Some("Space".to_string()));
    }

    #[test]
    fn test_no_substitution_without_dollar() {
        assert!(substitution_completions("usr/bin").is_none());
    }

    #[test]
    fn test_dir_items_filtered_by_prefix() {
        let items = dir_items("usr/", &HashSet::new());
        assert!(!items.is_empty());
        assert!(items.iter().all(|i| i.label.starts_with("usr/")));
        assert!(items
            .iter()
            .all(|i| i.kind == Some(CompletionItemKind::FOLDER)));
    }

    #[test]
    fn test_dir_items_leading_slash_ignored() {
        let items = dir_items("/usr/", &HashSet::new());
        assert!(items.iter().all(|i| i.label.starts_with("usr/")));
    }

    #[test]
    fn test_dir_items_excludes_taken() {
        let taken: HashSet<&str> = ["usr/bin/"].into_iter().collect();
        let items = dir_items("usr/", &taken);
        assert!(!items.iter().any(|i| i.label == "usr/bin/"));
    }
}
