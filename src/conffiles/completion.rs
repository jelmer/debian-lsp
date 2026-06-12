use std::collections::HashSet;
use std::path::Path;

use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use super::fields::CONFFILES_FLAGS;

/// Get completion items for a debian/conffiles file.
pub fn get_completions(
    source_text: &str,
    position: Position,
    debian_dir: Option<&Path>,
) -> Vec<CompletionItem> {
    let current_line = source_text
        .lines()
        .nth(position.line as usize)
        .unwrap_or("");
    let trimmed = current_line.trim();
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();

    let Some(prefix) = determine_prefix(&tokens, current_line) else {
        return Vec::new();
    };

    let mut items = Vec::new();

    if should_offer_remove_on_upgrade(&tokens) {
        items.push(remove_on_upgrade_item());
    }

    if let Some(paths) = collect_staging_paths(debian_dir, source_text, position, prefix) {
        items.extend(paths);
    }

    items
}

/// Determine the path prefix being typed, or None if no completions should be offered.
fn determine_prefix<'a>(tokens: &[&'a str], current_line: &str) -> Option<&'a str> {
    let ends_with_space = current_line.ends_with(char::is_whitespace);

    match tokens {
        t if t.len() > 2 => None,
        [_, _] if ends_with_space => None,
        [] => Some(""),
        [single] if single.starts_with('/') && ends_with_space => None,
        [single] if single.starts_with('/') => Some(single),
        [_flag] => Some(""),
        [_flag, path] => Some(path),
        _ => None,
    }
}

/// Check if remove-on-upgrade should be offered.
fn should_offer_remove_on_upgrade(tokens: &[&str]) -> bool {
    let flag = CONFFILES_FLAGS[0].0;
    match tokens {
        [] => true,
        [token] if !token.starts_with('/') && *token != flag => flag.starts_with(token),
        _ => false,
    }
}

/// Build the remove-on-upgrade completion item.
fn remove_on_upgrade_item() -> CompletionItem {
    let (label, detail) = CONFFILES_FLAGS[0];
    CompletionItem {
        label: label.to_string(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some(detail.to_string()),
        ..Default::default()
    }
}

/// Collect /etc/... paths from debhelper staging directories.
fn collect_staging_paths(
    debian_dir: Option<&Path>,
    source_text: &str,
    position: Position,
    prefix: &str,
) -> Option<Vec<CompletionItem>> {
    let debian_dir = debian_dir?;
    let already_listed = collect_already_listed(source_text, position);
    let mut items = Vec::new();
    let Ok(entries) = std::fs::read_dir(debian_dir) else {
        return Some(items);
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let etc_dir = path.join("etc");
        if etc_dir.is_dir() {
            collect_etc_files(&etc_dir, &etc_dir, &already_listed, prefix, &mut items);
        }
    }

    Some(items)
}

/// Collect paths already listed in the file, excluding the current line.
fn collect_already_listed(source_text: &str, position: Position) -> HashSet<String> {
    source_text
        .lines()
        .enumerate()
        .filter(|(i, _)| *i != position.line as usize)
        .filter_map(|(_, l)| {
            let l = l.trim();
            if l.starts_with('/') {
                Some(l.to_string())
            } else if let Some(path) = l.strip_prefix("remove-on-upgrade ") {
                Some(path.trim().to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Recursively collect files under etc_dir as absolute /etc/... paths.
fn collect_etc_files(
    base: &Path,
    current: &Path,
    already_listed: &HashSet<String>,
    prefix: &str,
    items: &mut Vec<CompletionItem>,
) {
    let Ok(entries) = std::fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_etc_files(base, &path, already_listed, prefix, items);
        } else if path.is_file() {
            if let Ok(rel) = path.strip_prefix(base) {
                let abs = format!("/etc/{}", rel.display());
                if !already_listed.contains(&abs) && abs.starts_with(prefix) {
                    items.push(CompletionItem {
                        label: abs,
                        kind: Some(CompletionItemKind::FILE),
                        detail: Some("Conffile".to_string()),
                        ..Default::default()
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_line_offers_remove_on_upgrade() {
        let items = get_completions("", Position::new(0, 0), None);
        assert!(items.iter().any(|i| i.label == "remove-on-upgrade"));
    }

    #[test]
    fn test_prefix_offers_remove_on_upgrade() {
        let items = get_completions("re", Position::new(0, 2), None);
        assert!(items.iter().any(|i| i.label == "remove-on-upgrade"));
    }

    #[test]
    fn test_remove_on_upgrade_not_offered_when_already_typed() {
        let items = get_completions("remove-on-upgrade", Position::new(0, 17), None);
        assert!(!items.iter().any(|i| i.label == "remove-on-upgrade"));
    }

    #[test]
    fn test_complete_path_with_space_offers_nothing() {
        let items = get_completions("/etc/foo/bar.conf ", Position::new(0, 18), None);
        assert!(items.is_empty());
    }

    #[test]
    fn test_no_source_root_returns_only_flag() {
        let items = get_completions("", Position::new(0, 0), None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "remove-on-upgrade");
    }

    #[test]
    fn test_two_complete_tokens_offers_nothing() {
        let items = get_completions("remove-on-upgrade /etc/foo ", Position::new(0, 27), None);
        assert!(items.is_empty());
    }
}
