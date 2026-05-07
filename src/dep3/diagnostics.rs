//! Diagnostics for DEP-3 patch headers.
//!
//! Operates only on the header portion of a patch — the unified diff
//! body is left to diff-lsp.

use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

use crate::position::text_range_to_lsp_range;

/// Generate diagnostics for a DEP-3 header. `header` is the parsed
/// deb822 tree of the header portion only (everything before the
/// first `---` / `diff ` / `Index:` line); `source_text` is the
/// whole patch buffer, needed to map rowan byte ranges back to LSP
/// `Position`s.
///
/// Currently surfaces field-name casing issues (e.g. `description` →
/// `Description`). Returns an empty vector if the header is empty.
pub fn get_diagnostics(header: &deb822_lossless::Deb822, source_text: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for paragraph in header.paragraphs() {
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

    /// Test helper: parse the DEP-3 header out of `text` and call
    /// `get_diagnostics` with it.
    fn run(text: &str) -> Vec<Diagnostic> {
        let header_end = dep3::lossless::header_end(text);
        let parsed = deb822_lossless::Deb822::parse(&text[..header_end]);
        get_diagnostics(&parsed.tree(), text)
    }

    #[test]
    fn lowercase_field_flagged() {
        let text = "author: alice\ndescription: bla\n";
        let diags = run(text);
        assert_eq!(diags.len(), 2);
        assert_eq!(diags[0].message, "Field name 'author' should be 'Author'");
        assert_eq!(
            diags[1].message,
            "Field name 'description' should be 'Description'"
        );
    }

    #[test]
    fn canonical_field_not_flagged() {
        assert_eq!(run("Author: alice\nDescription: bla\n").len(), 0);
    }

    #[test]
    fn unknown_field_not_flagged() {
        assert_eq!(run("Author: alice\nX-Custom: y\n").len(), 0);
    }

    #[test]
    fn bug_vendor_field_not_flagged() {
        assert_eq!(
            run("Author: alice\nBug-Debian: https://bugs.debian.org/1\n").len(),
            0
        );
    }

    #[test]
    fn diff_body_not_inspected() {
        // First field is in header — well, "wrong-case" is unknown so
        // not flagged. The point is: the diff line below is never
        // looked at.
        assert!(run("wrong-case: alice\n---\nthis-would-also-be-wrong: x\n").is_empty());
    }

    #[test]
    fn diff_body_after_known_field_not_inspected() {
        // `author` should be flagged once; `foo:` in the diff body is
        // not a field at all and must not appear.
        assert_eq!(run("author: alice\n---\nfoo: bar\n").len(), 1);
    }
}
