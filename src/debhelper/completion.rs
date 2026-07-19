//! Shared completion building blocks for debhelper files.

use std::collections::HashSet;

use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use crate::debhelper::parser::{parse_line, CursorContext, SubstitutionStart};

/// Common directories that packages are expected to install files into.
const COMMON_DIRS: &[&str] = &[
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

/// Substitution variables expanded in debhelper config files.
const SUBSTITUTION_VARS: &[(&str, &str)] = &[
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
    ("env:", "value of an environment variable"),
    ("Dollar", "A literal '$' character"),
    ("Newline", "A literal newline character"),
    ("Space", "A literal space character"),
    ("Tab", "A literal tab character"),
];

/// Completion items for the substitution the cursor is opening. After a bare
/// `$` the inserted text carries its own braces; after `${` they are already
/// typed, so only the name goes in.
pub(crate) fn substitution_completions(start: SubstitutionStart) -> Vec<CompletionItem> {
    let needs_braces = matches!(start, SubstitutionStart::Dollar);
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

/// Build directory completions from [`COMMON_DIRS`]
pub(crate) fn dir_items(prefix: &str, taken: &HashSet<&str>) -> Vec<CompletionItem> {
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

pub(crate) fn common_completions(cx: &CursorContext) -> Option<Vec<CompletionItem>> {
    if cx.in_comment {
        return Some(Vec::new());
    }
    match cx.substitution {
        Some(SubstitutionStart::Dollar) => {
            Some(substitution_completions(SubstitutionStart::Dollar))
        }
        Some(SubstitutionStart::Brace) => Some(substitution_completions(SubstitutionStart::Brace)),
        None => None,
    }
}

/// The completion shape every line-oriented debhelper file shares
pub(crate) fn get_completions(
    text: &str,
    position: Position,
    field: impl Fn(usize, &str) -> Vec<CompletionItem>,
) -> Vec<CompletionItem> {
    let line = text.lines().nth(position.line as usize).unwrap_or("");
    let offset = (position.character as usize).min(line.len());
    let cx = CursorContext::at(line, offset);

    if let Some(items) = common_completions(&cx) {
        return items;
    }
    field(cx.token_index, cx.prefix)
}

/// The entries listed on every line.
pub(crate) fn other_entries(text: &str, skip: usize) -> HashSet<&str> {
    text.lines()
        .enumerate()
        .filter(|&(i, _)| i != skip)
        .filter(|(_, l)| {
            let parsed = parse_line(l);
            parsed.comment.is_none() && !parsed.words.is_empty()
        })
        .map(|(_, l)| l.trim().trim_start_matches('/'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_items_filtered_by_prefix() {
        let items = dir_items("usr/", &HashSet::new());
        assert!(items.iter().all(|i| i.label.starts_with("usr/")));
        assert!(!items.iter().any(|i| i.label.starts_with("etc/")));
    }

    #[test]
    fn dir_items_ignores_a_leading_slash() {
        let items = dir_items("/usr/", &HashSet::new());
        assert!(items.iter().all(|i| i.label.starts_with("usr/")));
        assert!(!items.is_empty());
    }

    #[test]
    fn dir_items_excludes_taken() {
        let taken = HashSet::from(["usr/bin/"]);
        let items = dir_items("usr/", &taken);
        assert!(!items.iter().any(|i| i.label == "usr/bin/"));
    }

    #[test]
    fn dir_items_are_folders() {
        let items = dir_items("", &HashSet::new());
        assert!(items
            .iter()
            .all(|i| i.kind == Some(CompletionItemKind::FOLDER)));
    }

    #[test]
    fn a_bare_dollar_inserts_the_braces() {
        let items = substitution_completions(SubstitutionStart::Dollar);
        let item = items
            .iter()
            .find(|i| i.label == "${DEB_HOST_MULTIARCH}")
            .unwrap();
        assert_eq!(item.insert_text, Some("{DEB_HOST_MULTIARCH}".to_string()));
    }

    #[test]
    fn a_brace_inserts_only_the_name() {
        let items = substitution_completions(SubstitutionStart::Brace);
        let item = items.iter().find(|i| i.label == "${Space}").unwrap();
        assert_eq!(item.insert_text, Some("Space".to_string()));
    }

    #[test]
    fn offers_the_env_substitution() {
        let items = substitution_completions(SubstitutionStart::Dollar);
        assert!(items.iter().any(|i| i.label == "${env:}"));
    }

    #[test]
    fn common_completions_is_empty_in_a_comment() {
        let cx = CursorContext::at("# a comment", 11);
        assert_eq!(common_completions(&cx), Some(Vec::new()));
    }

    #[test]
    fn common_completions_offers_substitution_vars() {
        let cx = CursorContext::at("usr/lib/$", 9);
        let items = common_completions(&cx).unwrap();
        assert!(items.iter().any(|i| i.label == "${DEB_HOST_MULTIARCH}"));
    }

    #[test]
    fn common_completions_defers_on_a_plain_token() {
        let cx = CursorContext::at("usr/bi", 6);
        assert_eq!(common_completions(&cx), None);
    }

    #[test]
    fn get_completions_defers_to_the_field_callback() {
        let items = get_completions("usr etc", Position::new(0, 7), |index, prefix| {
            vec![CompletionItem {
                label: format!("{index}:{prefix}"),
                ..Default::default()
            }]
        });
        assert_eq!(items[0].label, "1:etc");
    }

    #[test]
    fn get_completions_skips_the_field_callback_in_a_comment() {
        let items = get_completions("# note", Position::new(0, 6), |_, _| {
            vec![CompletionItem {
                label: "unreachable".to_string(),
                ..Default::default()
            }]
        });
        assert!(items.is_empty());
    }

    #[test]
    fn get_completions_offers_substitutions_over_the_field_callback() {
        let items = get_completions("usr/$", Position::new(0, 5), |_, _| Vec::new());
        assert!(items.iter().any(|i| i.label == "${DEB_HOST_MULTIARCH}"));
    }

    #[test]
    fn other_entries_collects_the_rest_slash_stripped() {
        let entries = other_entries("usr/bin\n/etc\n# c\n\nusr/lib\n", 0);
        assert!(entries.contains("etc"));
        assert!(entries.contains("usr/lib"));
        assert!(!entries.contains("usr/bin")); // the skipped line
        assert!(!entries.iter().any(|e| e.starts_with('#') || e.is_empty()));
    }
}
