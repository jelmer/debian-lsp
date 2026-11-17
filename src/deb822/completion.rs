use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Documentation, Position};

use crate::position::Source;

/// How a field's value relates to web URLs, used to surface clickable links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldContent {
    /// No URL handling: the value is not expected to contain links.
    Plain,
    /// The entire value is a single URL (e.g. `Homepage`, `Vcs-Git`).
    Url,
    /// Free-form prose that may contain URLs embedded in the text (e.g.
    /// `Comment`, `Disclaimer`). Links are found by scanning the value.
    Prose,
}

/// A field definition for a deb822-based file format.
pub struct FieldInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub content: FieldContent,
}

impl FieldInfo {
    pub const fn new(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            content: FieldContent::Plain,
        }
    }

    /// Mark the whole value as a single URL.
    pub const fn url(mut self) -> Self {
        self.content = FieldContent::Url;
        self
    }

    /// Mark the value as free-form prose that may contain embedded URLs.
    pub const fn prose(mut self) -> Self {
        self.content = FieldContent::Prose;
        self
    }
}

/// Look up the [`FieldContent`] classification for a field by name
/// (case-insensitive). Returns [`FieldContent::Plain`] for unknown fields.
pub fn field_content(fields: &[FieldInfo], field_name: &str) -> FieldContent {
    let lowercase = field_name.to_lowercase();
    fields
        .iter()
        .find(|f| f.name.to_lowercase() == lowercase)
        .map(|f| f.content)
        .unwrap_or(FieldContent::Plain)
}

/// What kind of position the cursor is at in a deb822 document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CursorContext {
    /// Cursor is on a field key (the part before the colon).
    FieldKey,
    /// Cursor is on a field value (after the colon).
    FieldValue {
        field_name: String,
        value_prefix: String,
    },
    /// Cursor is at the start of a line where a new field could be added.
    StartOfLine,
}

/// Look up the standard (canonical) casing for a field name.
pub fn get_standard_field_name(fields: &[FieldInfo], field_name: &str) -> Option<&'static str> {
    let lowercase = field_name.to_lowercase();
    fields
        .iter()
        .find(|f| f.name.to_lowercase() == lowercase)
        .map(|f| f.name)
}

/// Look up the canonical name and description of a field (case-insensitive).
///
/// Shared by the LSP hover providers and the SCIP indexer.
pub fn field_description(
    fields: &[FieldInfo],
    field_name: &str,
) -> Option<(&'static str, &'static str)> {
    let lowercase = field_name.to_lowercase();
    fields
        .iter()
        .find(|f| f.name.to_lowercase() == lowercase)
        .map(|f| (f.name, f.description))
}

/// Generate field name completions from a list of field definitions.
pub fn get_field_completions(fields: &[FieldInfo]) -> Vec<CompletionItem> {
    get_field_completions_excluding(fields, |_| false)
}

/// Generate field name completions, skipping any field for which
/// `is_already_present` returns true.
///
/// Each deb822 field may appear at most once per paragraph, so the caller
/// passes a predicate that answers, for a candidate field name, whether the
/// paragraph already contains it. The field list is walked once and the
/// predicate is invoked only for fields we are about to emit, so no list of
/// existing names is collected up front.
pub fn get_field_completions_excluding(
    fields: &[FieldInfo],
    is_already_present: impl Fn(&str) -> bool,
) -> Vec<CompletionItem> {
    fields
        .iter()
        .filter(|field| !is_already_present(field.name))
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

/// Locate the paragraph whose existing fields should be excluded from field
/// completions at `position`, together with the range of the entry the cursor
/// is currently inside (so it can be preserved as a completion candidate).
///
/// A paragraph counts as "containing" the cursor when the cursor lies within
/// the paragraph's text range *or* immediately at its end - the latter covers
/// the typical case of typing a new field directly after the last entry, with
/// no intervening blank line. A blank-line separator between the paragraph
/// and the cursor disqualifies it.
///
/// Returns `None` when the cursor is not inside any paragraph, e.g. on a
/// blank line between paragraphs: the next field typed there would start a
/// brand new paragraph, so nothing should be filtered out.
fn paragraph_context_at(
    deb822: &deb822_lossless::Deb822,
    src: Source<'_>,
    position: Position,
) -> Option<(deb822_lossless::Paragraph, Option<text_size::TextRange>)> {
    let offset = src.try_position_to_offset(position)?;
    let paragraph = deb822
        .paragraph_at_position(offset)
        .or_else(|| deb822.paragraphs().find(|p| p.text_range().end() == offset))?;
    let current_entry_range = paragraph.entry_at_position(offset).map(|e| e.text_range());
    Some((paragraph, current_entry_range))
}

/// Return true if `paragraph` contains an entry with field name `field_name`
/// (case-insensitive), ignoring the entry at `exclude_range` (the one the
/// user is currently editing). Compares against the source text in `text`
/// via key ranges, so no `String` is allocated per entry.
fn paragraph_has_field(
    paragraph: &deb822_lossless::Paragraph,
    text: &str,
    field_name: &str,
    exclude_range: Option<text_size::TextRange>,
) -> bool {
    paragraph
        .entries()
        .filter(|e| Some(e.text_range()) != exclude_range)
        .filter_map(|e| e.key_range())
        .any(|r| {
            let start: usize = r.start().into();
            let end: usize = r.end().into();
            text[start..end].eq_ignore_ascii_case(field_name)
        })
}

/// Determine what kind of completion context the cursor is in.
///
/// Returns `None` for positions where no completions make sense
/// (e.g. continuation lines, comments, blank lines between paragraphs).
pub fn get_cursor_context(
    deb822: &deb822_lossless::Deb822,
    src: Source<'_>,
    position: Position,
) -> Option<CursorContext> {
    if src.text.is_empty() {
        return Some(CursorContext::StartOfLine);
    }

    let offset = src.try_position_to_offset(position)?;

    // If cursor is at column 0 of a new line, it's a position where a new
    // field can be started, not a continuation of the previous entry.
    if position.character == 0 {
        return Some(CursorContext::StartOfLine);
    }

    // Find the entry that contains the cursor offset. For incomplete entries
    // (no colon — the user is still typing a field name), the CST range
    // excludes the trailing newline, so we use an inclusive end check to
    // match when the cursor is right at the boundary.
    let entry = deb822
        .paragraphs()
        .flat_map(|p| p.entries().collect::<Vec<_>>())
        .find(|entry| {
            let r = entry.text_range();
            if entry.colon_range().is_none() {
                r.start() <= offset && offset <= r.end()
            } else {
                r.start() <= offset && offset < r.end()
            }
        });

    if let Some(entry) = entry {
        let field_name = entry.key()?;
        let colon_range = match entry.colon_range() {
            Some(r) => r,
            None => {
                // Entry has a key but no colon — the user is still typing
                // a field name. Treat as a field key.
                return Some(CursorContext::FieldKey);
            }
        };

        if offset < colon_range.start() {
            // Before the colon → on the field key
            return Some(CursorContext::FieldKey);
        }

        if offset < colon_range.end() {
            // On the colon itself → treat as field key
            return Some(CursorContext::FieldKey);
        }

        // After the colon → on the field value
        // Use the raw source text (not entry.value()) to extract the prefix,
        // because value_range() spans the raw source including newlines and
        // continuation-line indentation, while entry.value() strips those.
        let value_prefix = if let Some(value_range) = entry.value_range() {
            if offset <= value_range.start() {
                String::new()
            } else {
                let prefix_end = if offset < value_range.end() {
                    offset
                } else {
                    value_range.end()
                };
                let start: usize = value_range.start().into();
                let end: usize = prefix_end.into();
                let mut prefix_bytes = end.min(src.text.len());
                while !src.text.is_char_boundary(prefix_bytes) {
                    prefix_bytes -= 1;
                }
                src.text[start..prefix_bytes].to_string()
            }
        } else {
            String::new()
        };

        return Some(CursorContext::FieldValue {
            field_name,
            value_prefix,
        });
    }

    // Not on any entry — only offer field completions at column 0
    // (start of a line where a new field could be written).
    if position.character == 0 {
        Some(CursorContext::StartOfLine)
    } else {
        None
    }
}

/// Get completions for a deb822 document at the given cursor position.
///
/// If the cursor is on a field value, calls `value_completer` to get
/// value-specific completions (or returns empty if none are defined for
/// that field). If the cursor is not on a field value, returns field
/// name completions.
pub fn get_completions(
    deb822: &deb822_lossless::Deb822,
    src: Source<'_>,
    position: Position,
    fields: &[FieldInfo],
    value_completer: impl Fn(&str, &str) -> Vec<CompletionItem>,
) -> Vec<CompletionItem> {
    match get_cursor_context(deb822, src, position) {
        Some(CursorContext::FieldValue {
            field_name,
            value_prefix,
        }) => value_completer(&field_name, &value_prefix),
        Some(CursorContext::FieldKey | CursorContext::StartOfLine) => {
            let (paragraph, current_entry_range) =
                paragraph_context_at(deb822, src, position).unzip();
            let current_entry_range = current_entry_range.flatten();
            get_field_completions_excluding(fields, |name| match &paragraph {
                Some(p) => paragraph_has_field(p, src.text, name, current_entry_range),
                None => false,
            })
        }
        None => vec![],
    }
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
    fn test_get_cursor_context_on_value() {
        let text = "Source: test\nSection: py\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let idx = crate::position::LineIndex::new(text);

        let ctx = get_cursor_context(&deb822, Source::new(text, &idx), Position::new(1, 11))
            .expect("Should have context");
        assert_eq!(
            ctx,
            CursorContext::FieldValue {
                field_name: "Section".to_string(),
                value_prefix: "py".to_string(),
            }
        );
    }

    #[test]
    fn test_get_cursor_context_immediately_after_colon() {
        let text = "Section: py\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let idx = crate::position::LineIndex::new(text);

        let ctx = get_cursor_context(&deb822, Source::new(text, &idx), Position::new(0, 8))
            .expect("Should have context");
        assert_eq!(
            ctx,
            CursorContext::FieldValue {
                field_name: "Section".to_string(),
                value_prefix: "".to_string(),
            }
        );
    }

    #[test]
    fn test_get_cursor_context_on_field_key() {
        let text = "Source: test\nSection: py\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let idx = crate::position::LineIndex::new(text);

        let ctx = get_cursor_context(&deb822, Source::new(text, &idx), Position::new(1, 3))
            .expect("Should have context");
        assert_eq!(ctx, CursorContext::FieldKey);
    }

    #[test]
    fn test_get_cursor_context_empty_text() {
        let text = "";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let idx = crate::position::LineIndex::new(text);
        let ctx = get_cursor_context(&deb822, Source::new(text, &idx), Position::new(0, 0))
            .expect("Should have context");
        assert_eq!(ctx, CursorContext::StartOfLine);
    }

    #[test]
    fn test_get_completions_on_value_with_completer() {
        let text = "Source: te\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        let idx = crate::position::LineIndex::new(text);
        let completions = get_completions(
            &deb822,
            Source::new(text, &idx),
            Position::new(0, 10),
            TEST_FIELDS,
            |field, _prefix| {
                if field == "Source" {
                    vec![CompletionItem {
                        label: "test-value".to_string(),
                        ..Default::default()
                    }]
                } else {
                    vec![]
                }
            },
        );

        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].label, "test-value");
    }

    #[test]
    fn test_get_completions_falls_back_to_fields() {
        let text = "Source: test\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        // Cursor on field key area → no value context → field completions
        let idx = crate::position::LineIndex::new(text);
        let completions = get_completions(
            &deb822,
            Source::new(text, &idx),
            Position::new(0, 2),
            TEST_FIELDS,
            |_, _| vec![],
        );

        assert_eq!(completions.len(), 2);
        assert_eq!(completions[0].label, "Source");
        assert_eq!(completions[1].label, "Package");
    }

    #[test]
    fn test_get_completions_no_value_completions_returns_empty() {
        let text = "Source: test\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        // Cursor on value, completer returns empty → empty (not field completions)
        let idx = crate::position::LineIndex::new(text);
        let completions = get_completions(
            &deb822,
            Source::new(text, &idx),
            Position::new(0, 10),
            TEST_FIELDS,
            |_, _| vec![],
        );

        assert!(completions.is_empty());
    }

    #[test]
    fn test_get_cursor_context_multiline_value() {
        let text = "Build-Depends:\n debhelper-compat (= 13),\n pkg-co\n";
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();

        // Cursor at end of "pkg-co" on line 2, column 7
        let idx = crate::position::LineIndex::new(text);

        let ctx = get_cursor_context(&deb822, Source::new(text, &idx), Position::new(2, 7))
            .expect("Should have context");
        match ctx {
            CursorContext::FieldValue {
                field_name,
                value_prefix,
            } => {
                assert_eq!(field_name, "Build-Depends");
                // Should include the full raw value text up to cursor
                assert!(
                    value_prefix.contains("debhelper-compat"),
                    "prefix should contain prior relations: {:?}",
                    value_prefix
                );
                assert!(
                    value_prefix.ends_with("pkg-co"),
                    "prefix should end with partial name: {:?}",
                    value_prefix
                );
            }
            other => panic!("Expected FieldValue, got {:?}", other),
        }
    }

    #[test]
    fn test_get_cursor_context_partial_field_name_no_colon() {
        let text = "Source: test\nMai";
        let deb822 = deb822_lossless::Deb822::parse(text).tree();
        let idx = crate::position::LineIndex::new(text);

        let ctx = get_cursor_context(&deb822, Source::new(text, &idx), Position::new(1, 3))
            .expect("Should have context");
        assert_eq!(ctx, CursorContext::FieldKey);
    }

    #[test]
    fn test_get_cursor_context_empty_new_line_after_entry() {
        let text = "Source: test\n";
        let deb822 = deb822_lossless::Deb822::parse(text).tree();
        let idx = crate::position::LineIndex::new(text);

        let ctx = get_cursor_context(&deb822, Source::new(text, &idx), Position::new(1, 0))
            .expect("Should have context");
        assert_eq!(ctx, CursorContext::StartOfLine);
    }

    #[test]
    fn test_get_cursor_context_typing_single_char_on_new_line() {
        let text = "Source: test\nM";
        let deb822 = deb822_lossless::Deb822::parse(text).tree();
        let idx = crate::position::LineIndex::new(text);

        let ctx = get_cursor_context(&deb822, Source::new(text, &idx), Position::new(1, 1))
            .expect("Should have context");
        assert_eq!(ctx, CursorContext::FieldKey);
    }

    #[test]
    fn test_get_cursor_context_substvar_after_comma() {
        let text = "Depends: gpg,${misc:\n";
        let deb822 = deb822_lossless::Deb822::parse(text).tree();
        let idx = crate::position::LineIndex::new(text);

        let ctx = get_cursor_context(&deb822, Source::new(text, &idx), Position::new(0, 20))
            .expect("Should have context");
        match ctx {
            CursorContext::FieldValue {
                field_name,
                value_prefix,
            } => {
                assert_eq!(field_name, "Depends");
                assert_eq!(value_prefix, "gpg,${misc:");
            }
            other => panic!("Expected FieldValue, got {:?}", other),
        }
    }

    #[test]
    fn test_get_cursor_context_substvar_multiline() {
        let text = "Depends:\n gpg,${misc:\n";
        let deb822 = deb822_lossless::Deb822::parse(text).tree();
        // Line 1: " gpg,${misc:\n", cursor at col 12 (after last ':')
        let idx = crate::position::LineIndex::new(text);

        let ctx = get_cursor_context(&deb822, Source::new(text, &idx), Position::new(1, 12))
            .expect("Should have context");
        match ctx {
            CursorContext::FieldValue {
                field_name,
                value_prefix,
            } => {
                assert_eq!(field_name, "Depends");
                // value_prefix should NOT include the continuation-line indent
                assert_eq!(value_prefix, "gpg,${misc:");
            }
            other => panic!("Expected FieldValue, got {:?}", other),
        }
    }

    #[test]
    fn test_get_cursor_context_substvar_after_comma_space() {
        let text = "Depends: gpg, ${misc:\n";
        let deb822 = deb822_lossless::Deb822::parse(text).tree();
        let idx = crate::position::LineIndex::new(text);

        let ctx = get_cursor_context(&deb822, Source::new(text, &idx), Position::new(0, 21))
            .expect("Should have context");
        match ctx {
            CursorContext::FieldValue {
                field_name,
                value_prefix,
            } => {
                assert_eq!(field_name, "Depends");
                assert_eq!(value_prefix, "gpg, ${misc:");
            }
            other => panic!("Expected FieldValue, got {:?}", other),
        }
    }

    #[test]
    fn test_partial_field_between_existing_fields() {
        let text = "Source: debian-codemods\nSection: devel\nHomepa\nPriority: optional\n";
        let deb822 = deb822_lossless::Deb822::parse(text).tree();
        let idx = crate::position::LineIndex::new(text);
        let ctx = get_cursor_context(&deb822, Source::new(text, &idx), Position::new(2, 6));
        assert!(ctx.is_some(), "Should have context");
        assert_eq!(ctx.unwrap(), CursorContext::FieldKey);
    }

    #[test]
    fn test_field_completions_omit_existing_fields_in_paragraph() {
        let text = "Source: hello\n";
        let deb822 = deb822_lossless::Deb822::parse(text).tree();
        let idx = crate::position::LineIndex::new(text);
        // Cursor on the blank line following "Source: hello\n" where the next
        // field in the same paragraph would be typed.
        let completions = get_completions(
            &deb822,
            Source::new(text, &idx),
            Position::new(1, 0),
            TEST_FIELDS,
            |_, _| vec![],
        );

        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["Package"]);
    }

    #[test]
    fn test_field_completions_include_current_entry_being_edited() {
        // User is partway through editing a field name that happens to match
        // an existing field name in the fields list - it should still be
        // offered (we only filter *other* entries in the paragraph).
        let text = "Sou\n";
        let deb822 = deb822_lossless::Deb822::parse(text).tree();
        let idx = crate::position::LineIndex::new(text);
        let completions = get_completions(
            &deb822,
            Source::new(text, &idx),
            Position::new(0, 3),
            TEST_FIELDS,
            |_, _| vec![],
        );
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"Source"));
        assert!(labels.contains(&"Package"));
    }

    #[test]
    fn test_field_completions_not_filtered_across_paragraphs() {
        // A blank line separates paragraphs; fields from the previous
        // paragraph must not be filtered out of completions for a new one.
        let text = "Source: hello\n\n";
        let deb822 = deb822_lossless::Deb822::parse(text).tree();
        let idx = crate::position::LineIndex::new(text);
        let completions = get_completions(
            &deb822,
            Source::new(text, &idx),
            Position::new(2, 0),
            TEST_FIELDS,
            |_, _| vec![],
        );
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"Source"));
        assert!(labels.contains(&"Package"));
    }
}
