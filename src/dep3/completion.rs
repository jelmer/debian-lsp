//! Completions for DEP-3 patch headers.
//!
//! Active only when the cursor is in the header portion of the file.
//! Completions in the unified-diff body are left to diff-lsp.

use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use super::fields::DEP3_FIELDS;

/// Get completion items for the DEP-3 header at `position`. Returns
/// `Vec::new()` if the cursor is in the diff body or the file has no
/// header at all.
pub fn get_completions(source_text: &str, position: Position) -> Vec<CompletionItem> {
    if !super::is_in_dep3_header(source_text, position) {
        return Vec::new();
    }
    let header_end = dep3::lossless::header_end(source_text);
    let header_text = &source_text[..header_end];
    let parsed = deb822_lossless::Deb822::parse(header_text);
    let deb822 = parsed.tree();
    crate::deb822::completion::get_completions(
        &deb822,
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

    #[test]
    fn field_name_completions_at_start_of_line_in_header() {
        let text = "Author: alice\n\n";
        let completions = get_completions(text, Position::new(1, 0));
        // Should contain the canonical DEP-3 fields.
        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"Description"));
        assert!(labels.contains(&"Forwarded"));
        assert!(labels.contains(&"Last-Update"));
    }

    #[test]
    fn no_completions_in_diff_body() {
        let text = "Author: alice\n---\n@@ -1 +1 @@\n";
        let completions = get_completions(text, Position::new(2, 0));
        assert!(completions.is_empty());
    }

    #[test]
    fn forwarded_value_enum_completions() {
        let text = "Forwarded: \n";
        // Cursor is right after "Forwarded: " on line 0.
        let completions = get_completions(text, Position::new(0, 11));
        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"yes"));
        assert!(labels.contains(&"no"));
        assert!(labels.contains(&"not-needed"));
    }

    #[test]
    fn origin_category_enum_completions() {
        let text = "Origin: \n";
        let completions = get_completions(text, Position::new(0, 8));
        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"upstream"));
        assert!(labels.contains(&"backport"));
        assert!(labels.contains(&"vendor"));
    }
}
