use tower_lsp_server::ls_types::{CompletionItem, Position, Uri};

/// Get completion items for a debian/tests/control file
pub fn get_completions(_uri: &Uri, _position: Position) -> Vec<CompletionItem> {
    // TODO: Implement completions for debian/tests/control
    // For now, return empty - we'll add field completions later
    // when we have a dedicated debian-tests crate
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_completions_returns_empty_for_now() {
        let uri = "file:///debian/tests/control".parse().unwrap();
        let position = Position::new(0, 0);
        let completions = get_completions(&uri, position);

        // For now, should return empty
        assert!(completions.is_empty());
    }
}
