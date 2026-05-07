//! Diagnostics for DEP-3 patch headers.
//!
//! Operates only on the header portion of a patch — the unified diff
//! body is left to diff-lsp.

use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

use crate::position::text_range_to_lsp_range;

/// Generate diagnostics for the DEP-3 header at the top of `source_text`.
/// Currently surfaces field-name casing issues (e.g. `description` →
/// `Description`). Returns an empty vector if there is no header to
/// inspect.
pub fn get_diagnostics(source_text: &str) -> Vec<Diagnostic> {
    let header_end = dep3::lossless::header_end(source_text);
    if header_end == 0 {
        return Vec::new();
    }
    let header_text = &source_text[..header_end];
    let parsed = deb822_lossless::Deb822::parse(header_text);
    let deb822 = parsed.tree();

    let mut diags = Vec::new();
    for paragraph in deb822.paragraphs() {
        for entry in paragraph.entries() {
            let Some(key) = entry.key() else {
                continue;
            };
            // `Bug-<Vendor>` is valid DEP-3 (vendor-specific extension);
            // skip casing checks against the canonical-name table for
            // these. Only flag `Bug-` if the vendor part itself looks
            // mis-cased — out of scope for now.
            if key.starts_with("Bug-") {
                continue;
            }
            let Some(canonical) = super::get_standard_field_name(&key) else {
                continue; // unknown field; not a casing issue
            };
            if key == canonical {
                continue;
            }
            let Some(field_range) = entry.key_range() else {
                continue;
            };
            let lsp_range = text_range_to_lsp_range(source_text, field_range);
            diags.push(Diagnostic {
                range: lsp_range,
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(NumberOrString::String("field-casing".to_string())),
                source: Some("debian-lsp".to_string()),
                message: format!("Field name '{}' should be '{}'", key, canonical),
                ..Default::default()
            });
        }
    }
    diags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercase_field_flagged() {
        let text = "author: alice\ndescription: bla\n";
        let diags = get_diagnostics(text);
        assert_eq!(diags.len(), 2);
        assert_eq!(diags[0].message, "Field name 'author' should be 'Author'");
        assert_eq!(
            diags[1].message,
            "Field name 'description' should be 'Description'"
        );
    }

    #[test]
    fn canonical_field_not_flagged() {
        let text = "Author: alice\nDescription: bla\n";
        assert_eq!(get_diagnostics(text).len(), 0);
    }

    #[test]
    fn unknown_field_not_flagged() {
        let text = "Author: alice\nX-Custom: y\n";
        assert_eq!(get_diagnostics(text).len(), 0);
    }

    #[test]
    fn bug_vendor_field_not_flagged() {
        let text = "Author: alice\nBug-Debian: https://bugs.debian.org/1\n";
        assert_eq!(get_diagnostics(text).len(), 0);
    }

    #[test]
    fn diff_body_not_inspected() {
        let text = "wrong-case: alice\n---\nthis-would-also-be-wrong: x\n";
        let diags = get_diagnostics(text);
        // First field is in header — well, "wrong-case" is unknown so
        // not flagged. The point is: the diff line below is never
        // looked at.
        assert!(diags.is_empty());
    }

    #[test]
    fn diff_body_after_known_field_not_inspected() {
        let text = "author: alice\n---\nfoo: bar\n";
        let diags = get_diagnostics(text);
        // `author` should be flagged once; `foo:` in the diff body is
        // not a field at all and must not appear.
        assert_eq!(diags.len(), 1);
    }
}
