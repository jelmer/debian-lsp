use text_size::TextRange;
use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, Documentation, Position,
};

/// A field definition for a deb822-based file format.
pub struct FieldInfo {
    pub name: &'static str,
    pub description: &'static str,
}

impl FieldInfo {
    pub const fn new(name: &'static str, description: &'static str) -> Self {
        Self { name, description }
    }
}

/// Completion context: the field name and value prefix at the cursor position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionContext {
    pub field_name: String,
    pub value_prefix: String,
}

/// Look up the standard (canonical) casing for a field name.
pub fn get_standard_field_name(fields: &[FieldInfo], field_name: &str) -> Option<&'static str> {
    let lowercase = field_name.to_lowercase();
    fields
        .iter()
        .find(|f| f.name.to_lowercase() == lowercase)
        .map(|f| f.name)
}

/// Generate field name completions from a list of field definitions.
pub fn get_field_completions(fields: &[FieldInfo]) -> Vec<CompletionItem> {
    fields
        .iter()
        .map(|field| CompletionItem {
            label: field.name.to_string(),
            kind: Some(CompletionItemKind::FIELD),
            detail: Some(field.description.to_string()),
            documentation: Some(Documentation::String(field.description.to_string())),
            insert_text: Some(format!("{}: ", field.name)),
            ..Default::default()
        })
        .collect()
}

/// Extract the completion context (field name + value prefix) at a cursor
/// position in a deb822 document.
///
/// Returns `None` if the cursor is not positioned on a field value.
pub fn get_completion_context(
    deb822: &deb822_lossless::Deb822,
    source_text: &str,
    position: Position,
) -> Option<CompletionContext> {
    if source_text.is_empty() {
        return None;
    }

    let offset = crate::position::try_position_to_offset(source_text, position)?;
    let text_len = text_size::TextSize::try_from(source_text.len()).ok()?;

    let query_range = if offset >= text_len {
        if text_len == text_size::TextSize::from(0) {
            return None;
        }
        TextRange::new(text_len - text_size::TextSize::from(1), text_len)
    } else {
        TextRange::new(offset, offset + text_size::TextSize::from(1))
    };

    let entry = deb822
        .paragraphs()
        .flat_map(|p| p.entries().collect::<Vec<_>>())
        .find(|entry| {
            let r = entry.text_range();
            r.start() < query_range.end() && query_range.start() < r.end()
        })?;

    let field_name = entry.key()?;
    let colon_range = entry.colon_range()?;

    // Only offer value completions when cursor is at or after the ':' separator.
    if offset < colon_range.end() {
        return None;
    }

    let value_prefix = if let Some(value_range) = entry.value_range() {
        if offset <= value_range.start() {
            String::new()
        } else {
            let prefix_end = if offset < value_range.end() {
                offset
            } else {
                value_range.end()
            };
            let prefix_len: usize = (prefix_end - value_range.start()).into();
            let value = entry.value();
            let mut prefix_bytes = prefix_len.min(value.len());
            while !value.is_char_boundary(prefix_bytes) {
                prefix_bytes -= 1;
            }
            value[..prefix_bytes].to_string()
        }
    } else {
        String::new()
    };

    Some(CompletionContext {
        field_name,
        value_prefix,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_FIELDS: &[FieldInfo] = &[
        FieldInfo::new("Source", "Name of the source package"),
        FieldInfo::new("Package", "Binary package name"),
    ];

    #[test]
    fn test_get_field_completions() {
        let completions = get_field_completions(TEST_FIELDS);

        assert_eq!(completions.len(), 2);
        for completion in &completions {
            assert_eq!(completion.kind, Some(CompletionItemKind::FIELD));
            assert!(completion.detail.is_some());
            assert!(completion.documentation.is_some());
            assert!(completion.insert_text.as_ref().unwrap().ends_with(": "));
        }
        assert_eq!(completions[0].label, "Source");
        assert_eq!(completions[1].label, "Package");
    }

    #[test]
    fn test_get_completion_context_on_value() {
        let text = "Source: test\nSection: py\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let ctx = get_completion_context(&deb822, text, Position::new(1, 11))
            .expect("Should have context");
        assert_eq!(ctx.field_name, "Section");
        assert_eq!(ctx.value_prefix, "py");
    }

    #[test]
    fn test_get_completion_context_immediately_after_colon() {
        let text = "Section: py\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let ctx = get_completion_context(&deb822, text, Position::new(0, 8))
            .expect("Should have context");
        assert_eq!(ctx.field_name, "Section");
        assert_eq!(ctx.value_prefix, "");
    }

    #[test]
    fn test_get_completion_context_none_in_field_key() {
        let text = "Source: test\nSection: py\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let ctx = get_completion_context(&deb822, text, Position::new(1, 3));
        assert!(ctx.is_none());
    }

    #[test]
    fn test_get_completion_context_empty_text() {
        let text = "";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let ctx = get_completion_context(&deb822, text, Position::new(0, 0));
        assert!(ctx.is_none());
    }
}
