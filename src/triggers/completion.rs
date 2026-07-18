use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use crate::triggers::parser::CursorContext;

/// The directives a triggers control file may start a line with, from
/// deb-triggers(5).
pub(crate) const DIRECTIVES: &[(&str, &str)] = &[
    ("interest", "Wait on the named trigger"),
    (
        "interest-await",
        "Wait on the named trigger (explicit await)",
    ),
    (
        "interest-noawait",
        "Note interest in the trigger without waiting",
    ),
    ("activate", "Fire the named trigger in other packages"),
    ("activate-await", "Fire the named trigger (explicit await)"),
    ("activate-noawait", "Fire the named trigger without waiting"),
];

/// Completions for a triggers file at the given cursor position.
pub fn get_completions(text: &str, position: Position) -> Vec<CompletionItem> {
    let line = text.lines().nth(position.line as usize).unwrap_or("");
    let offset = (position.character as usize).min(line.len());
    let cx = CursorContext::at(line, offset);

    if cx.in_comment || cx.token_index != 0 {
        return Vec::new();
    }

    DIRECTIVES
        .iter()
        .filter(|(name, _)| name.starts_with(cx.prefix))
        .map(|&(name, detail)| CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some(detail.to_string()),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_directives_on_an_empty_line() {
        let items = get_completions("\n", Position::new(0, 0));
        assert!(items.iter().any(|i| i.label == "interest"));
        assert!(items.iter().any(|i| i.label == "activate-noawait"));
        assert!(items
            .iter()
            .all(|i| i.kind == Some(CompletionItemKind::KEYWORD)));
    }

    #[test]
    fn filters_by_prefix() {
        let items = get_completions("inter\n", Position::new(0, 5));
        assert!(items.iter().all(|i| i.label.starts_with("inter")));
        assert!(!items.iter().any(|i| i.label.starts_with("activate")));
    }

    #[test]
    fn nothing_on_the_trigger_token() {
        let items = get_completions("interest /usr/share/foo", Position::new(0, 23));
        assert!(items.is_empty());
    }

    #[test]
    fn nothing_in_a_whole_line_comment() {
        let items = get_completions("# interest\n", Position::new(0, 10));
        assert!(items.is_empty());
    }

    #[test]
    fn nothing_in_a_comment_following_a_directive() {
        let items = get_completions("interest foo # inter\n", Position::new(0, 20));
        assert!(items.is_empty());
    }

    #[test]
    fn no_substitution_completion() {
        let items = get_completions("$\n", Position::new(0, 1));
        assert!(!items.iter().any(|i| i.label.starts_with("${")));
    }
}
