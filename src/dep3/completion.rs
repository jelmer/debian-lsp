//! Completions for DEP-3 patch headers.
//!
//! Active only when the cursor is in the header portion of the file.
//! Completions in the unified-diff body are left to diff-lsp.

use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use super::fields::DEP3_FIELDS;

/// Get completion items for a DEP-3 header at `position`. `header`
/// is the parsed deb822 of the header portion only; `header_end` is
/// the byte offset where the diff body begins. Returns `Vec::new()`
/// if the cursor is in the diff body.
pub fn get_completions(
    header: &deb822_lossless::Deb822,
    header_end: usize,
    source_text: &str,
    position: Position,
) -> Vec<CompletionItem> {
    if !super::is_in_dep3_header(source_text, header_end, position) {
        return Vec::new();
    }
    let header_text = &source_text[..header_end];
    crate::deb822::completion::get_completions(
        header,
        header_text,
        position,
        DEP3_FIELDS,
        value_completions,
    )
}

/// Field-value completions for fields with a small enumerated value
/// space. `Forwarded:` and `Origin:` (the category prefix) are the
/// useful ones; everything else returns no value completions.
fn value_completions(field_name: &str, value_prefix: &str) -> Vec<CompletionItem> {
    let candidates: &[(&str, &str)] = match field_name {
        "Forwarded" => &[
            (
                "yes",
                "The patch has been forwarded upstream (followed by a URL or reference).",
            ),
            ("no", "The patch has not been forwarded upstream."),
            (
                "not-needed",
                "The patch is Debian-specific and doesn't need forwarding.",
            ),
        ],
        // Origin's first comma-separated component is the category;
        // suggest those when the user is at the start of the value.
        "Origin" if !value_prefix.contains(',') => &[
            ("upstream", "Cherry-picked from the upstream VCS."),
            (
                "backport",
                "An upstream patch that had to be modified to apply to this version.",
            ),
            (
                "vendor",
                "Created by Debian or another distribution vendor.",
            ),
            ("other", "Doesn't fit any of the above categories."),
        ],
        _ => return Vec::new(),
    };
    candidates
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

    fn run(text: &str, position: Position) -> Vec<CompletionItem> {
        let header_end = dep3::lossless::header_end(text);
        let parsed = deb822_lossless::Deb822::parse(&text[..header_end]);
        get_completions(&parsed.tree(), header_end, text, position)
    }

    #[test]
    fn field_name_completions_at_start_of_line_in_header() {
        let completions = run("Author: alice\n\n", Position::new(1, 0));
        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"Description"));
        assert!(labels.contains(&"Forwarded"));
        assert!(labels.contains(&"Last-Update"));
    }

    #[test]
    fn no_completions_in_diff_body() {
        assert!(run("Author: alice\n---\n@@ -1 +1 @@\n", Position::new(2, 0)).is_empty());
    }

    #[test]
    fn forwarded_value_enum_completions() {
        // Cursor is right after "Forwarded: " on line 0.
        let completions = run("Forwarded: \n", Position::new(0, 11));
        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"yes"));
        assert!(labels.contains(&"no"));
        assert!(labels.contains(&"not-needed"));
    }

    #[test]
    fn origin_category_enum_completions() {
        let completions = run("Origin: \n", Position::new(0, 8));
        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"upstream"));
        assert!(labels.contains(&"backport"));
        assert!(labels.contains(&"vendor"));
    }
}
