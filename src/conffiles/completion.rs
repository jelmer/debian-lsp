use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use super::REMOVE_ON_UPGRADE;

/// Get completion items for a debian/conffiles file.
pub fn get_completions(source_text: &str, position: Position) -> Vec<CompletionItem> {
    let current_line = source_text
        .lines()
        .nth(position.line as usize)
        .unwrap_or("");
    let trimmed = current_line.trim();
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();

    if !should_offer_remove_on_upgrade(&tokens) {
        return Vec::new();
    }

    vec![remove_on_upgrade_item()]
}

/// Check if remove-on-upgrade should be offered.
fn should_offer_remove_on_upgrade(tokens: &[&str]) -> bool {
    let flag = REMOVE_ON_UPGRADE;
    match tokens {
        [] => true,
        [token] if !token.starts_with('/') && *token != flag => flag.starts_with(token),
        _ => false,
    }
}

/// Build the remove-on-upgrade completion item.
fn remove_on_upgrade_item() -> CompletionItem {
    CompletionItem {
        label: REMOVE_ON_UPGRADE.to_string(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some("Remove this file when the package is upgraded".to_string()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_line_offers_remove_on_upgrade() {
        let items = get_completions("", Position::new(0, 0));
        assert!(items.iter().any(|i| i.label == "remove-on-upgrade"));
    }

    #[test]
    fn test_prefix_offers_remove_on_upgrade() {
        let items = get_completions("re", Position::new(0, 2));
        assert!(items.iter().any(|i| i.label == "remove-on-upgrade"));
    }

    #[test]
    fn test_remove_on_upgrade_not_offered_when_already_typed() {
        let items = get_completions("remove-on-upgrade", Position::new(0, 17));
        assert!(!items.iter().any(|i| i.label == "remove-on-upgrade"));
    }

    #[test]
    fn test_absolute_path_offers_nothing() {
        let items = get_completions("/etc/foo/bar.conf", Position::new(0, 17));
        assert!(items.is_empty());
    }

    #[test]
    fn test_complete_path_with_space_offers_nothing() {
        let items = get_completions("/etc/foo/bar.conf ", Position::new(0, 18));
        assert!(items.is_empty());
    }

    #[test]
    fn test_empty_line_returns_only_flag() {
        let items = get_completions("", Position::new(0, 0));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "remove-on-upgrade");
    }

    #[test]
    fn test_two_complete_tokens_offers_nothing() {
        let items = get_completions("remove-on-upgrade /etc/foo ", Position::new(0, 27));
        assert!(items.is_empty());
    }
}
