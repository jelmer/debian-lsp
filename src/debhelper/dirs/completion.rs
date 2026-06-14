use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

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

/// Substitution variables available in debhelper config files.
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
    ("Dollar", "A literal '$' character"),
    ("Newline", "A literal newline character"),
    ("Space", "A literal space character"),
    ("Tab", "A literal tab character"),
];

/// Get completions for a debian/dirs file at the given cursor position.
pub fn get_completions(text: &str, position: Position) -> Vec<CompletionItem> {
    let current_line = text.lines().nth(position.line as usize).unwrap_or("");

    let char_idx = (position.character as usize).min(current_line.len());
    let before_cursor = current_line.get(..char_idx).unwrap_or(current_line);

    if before_cursor.ends_with("${") {
        return substitution_var_items(false);
    }
    if before_cursor.ends_with('$') {
        return substitution_var_items(true);
    }

    let trimmed = current_line.trim_start();

    // Strip a leading slash so prefix matching works for both styles.
    let prefix = trimmed.trim_start_matches('/');

    // Collect already-listed directories to avoid suggesting duplicates.
    let existing: std::collections::HashSet<&str> = text
        .lines()
        .enumerate()
        .filter(|(i, _)| *i != position.line as usize)
        .map(|(_, l)| l.trim().trim_start_matches('/'))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    COMMON_DIRS
        .iter()
        .filter(|&&dir| dir.starts_with(prefix) && !existing.contains(dir))
        .map(|&dir| CompletionItem {
            label: dir.to_string(),
            kind: Some(CompletionItemKind::FOLDER),
            ..Default::default()
        })
        .collect()
}

/// Build completion items for debhelper substitution variables.
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
