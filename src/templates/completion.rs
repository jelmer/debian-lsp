use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use super::fields::{TEMPLATES_FIELDS, TEMPLATE_TYPES};
use crate::position::{LineIndex, Source};

/// Get completion items for a debconf templates.
pub fn get_completions(text: &str, position: Position) -> Vec<CompletionItem> {
    let deb822 = deb822_lossless::Deb822::parse(text).tree();
    let idx = LineIndex::new(text);
    let src = Source::new(text, &idx);
    crate::deb822::completion::get_completions(
        &deb822,
        src,
        position,
        TEMPLATES_FIELDS,
        value_completions,
    )
}

/// Field-value completions.
fn value_completions(field_name: &str, value_prefix: &str) -> Vec<CompletionItem> {
    if !field_name.eq_ignore_ascii_case("Type") {
        return Vec::new();
    }
    TEMPLATE_TYPES
        .iter()
        .filter(|(label, _)| label.starts_with(value_prefix))
        .map(|(label, doc)| CompletionItem {
            label: (*label).to_string(),
            kind: Some(CompletionItemKind::ENUM_MEMBER),
            detail: Some((*doc).to_string()),
            insert_text: Some((*label).to_string()),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(text: &str, position: Position) -> Vec<String> {
        get_completions(text, position)
            .into_iter()
            .map(|c| c.label)
            .collect()
    }

    #[test]
    fn field_names_at_start_of_line() {
        let labels = labels("Template: foo/bar\n\n", Position::new(1, 0));
        assert!(labels.contains(&"Type".to_string()));
        assert!(labels.contains(&"Description".to_string()));
        assert!(labels.contains(&"Choices".to_string()));
    }

    #[test]
    fn type_value_enum_completions() {
        let labels = labels("Type: \n", Position::new(0, 6));
        assert!(labels.contains(&"boolean".to_string()));
        assert!(labels.contains(&"select".to_string()));
        assert!(labels.contains(&"multiselect".to_string()));
    }

    #[test]
    fn type_value_filtered_by_prefix() {
        let labels = labels("Type: se\n", Position::new(0, 8));
        assert_eq!(labels, vec!["select".to_string()]);
    }

    #[test]
    fn no_value_completions_for_other_fields() {
        assert!(labels("Default: \n", Position::new(0, 9)).is_empty());
    }
}
