//! Spell-checking for prose fields in DEP-3 patch headers.
//!
//! The generic deb822 checker lives in [`crate::spelling::deb822`]; this module
//! supplies the DEP-3 wiring: which header fields hold free-form prose. Only the
//! `Description` field (and its `Subject` alias) is checked. Author, Origin,
//! Bug, Forwarded and the like hold names, URLs and identifiers, so they are
//! left alone to avoid false positives.

use tower_lsp_server::ls_types::{CodeActionOrCommand, Diagnostic, Uri};

use crate::position::Source;
use crate::spelling::deb822::deb822_findings;
use crate::spelling::{make_actions, make_diagnostic, LocatedFinding};

/// Field names in a DEP-3 header whose values are free-form prose worth
/// spell-checking.
fn is_prose_field(field_name: &str) -> bool {
    field_name.eq_ignore_ascii_case("Description") || field_name.eq_ignore_ascii_case("Subject")
}

/// Find all spelling mistakes in the prose fields of a DEP-3 patch header, each
/// mapped to its source range. `header` is the parsed deb822 header, whose token
/// ranges are absolute file offsets since the header starts at the top of the
/// file.
fn dep3_findings(header: &deb822_lossless::Deb822, src: Source<'_>) -> Vec<LocatedFinding> {
    deb822_findings(header, src, is_prose_field)
}

/// Produce spelling diagnostics for the prose fields of a DEP-3 patch header.
pub fn dep3_diagnostics(header: &deb822_lossless::Deb822, src: Source<'_>) -> Vec<Diagnostic> {
    dep3_findings(header, src)
        .into_iter()
        .map(|located| make_diagnostic(located.range, &located.finding))
        .collect()
}

/// Produce quick-fix code actions for spelling mistakes in a DEP-3 patch header.
/// One action per suggested correction.
///
/// When `diagnostics` is non-empty (the client requested fixes for specific
/// diagnostics), only actions whose range matches one of them are emitted, so
/// the quickfix doesn't appear for every nearby squiggle.
pub fn dep3_actions(
    uri: &Uri,
    header: &deb822_lossless::Deb822,
    src: Source<'_>,
    diagnostics: &[Diagnostic],
) -> Vec<CodeActionOrCommand> {
    make_actions(uri, dep3_findings(header, src), diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;
    use tower_lsp_server::ls_types::{CodeAction, CodeActionKind, NumberOrString, Position, Range};

    fn dep3_diags(content: &str) -> Vec<Diagnostic> {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/patches/foo.patch").unwrap();
        let file = workspace.update_file(url, content.to_string());
        let (parsed, _) = workspace.get_parsed_dep3_header(file);
        let text = workspace.source_text(file);
        let idx = workspace.get_line_index(file);
        let src = Source::new(&text, &idx);
        dep3_diagnostics(&parsed.tree(), src)
    }

    #[test]
    fn test_dep3_diagnostics_description_typo() {
        let content =
            "Description: Fix a libary lookup bug\nAuthor: Alice\n---\n@@ -1 +1 @@\n-x\n+y\n";
        let diags = dep3_diags(content);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "\"libary\" should be \"library\"");
        // "libary" starts at column 19 on the Description line (line 0).
        assert_eq!(
            diags[0].range,
            Range::new(Position::new(0, 19), Position::new(0, 25))
        );
    }

    #[test]
    fn test_dep3_diagnostics_subject_alias() {
        let content = "Subject: Correct teh spelling\nAuthor: Alice\n---\n@@ -1 +1 @@\n";
        let diags = dep3_diags(content);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "\"teh\" should be \"the\"");
    }

    #[test]
    fn test_dep3_diagnostics_multiline_description() {
        // Typo on a continuation line: the joined-value offset must map back
        // through the '\n' separator to the right source position.
        let content =
            "Description: short synopsis\n A longer body that recieves input.\n---\n@@ -1 +1 @@\n";
        let diags = dep3_diags(content);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "\"recieves\" should be \"receives\"");
        assert_eq!(diags[0].range.start.line, 1);
    }

    #[test]
    fn test_dep3_diagnostics_skips_non_prose_fields() {
        // A typo-shaped token in Author/Origin/Bug must not be flagged, nor
        // anything in the diff body below the header.
        let content = "Description: clean text\nAuthor: teh libary maintainer\nOrigin: https://example.com/libary\n---\n@@ -1 +1 @@\n-teh libary\n+the library\n";
        assert_eq!(dep3_diags(content), vec![]);
    }

    fn dep3_acts(content: &str, diagnostics: &[Diagnostic]) -> Vec<CodeAction> {
        let mut workspace = Workspace::new();
        let url: Uri = str::parse("file:///debian/patches/foo.patch").unwrap();
        let file = workspace.update_file(url.clone(), content.to_string());
        let (parsed, _) = workspace.get_parsed_dep3_header(file);
        let text = workspace.source_text(file);
        let idx = workspace.get_line_index(file);
        let src = Source::new(&text, &idx);
        dep3_actions(&url, &parsed.tree(), src, diagnostics)
            .into_iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(action) => action,
                CodeActionOrCommand::Command(_) => panic!("expected CodeAction"),
            })
            .collect()
    }

    #[test]
    fn test_dep3_actions_quickfix() {
        let content = "Description: Fix a libary lookup bug\n---\n@@ -1 +1 @@\n";
        let actions = dep3_acts(content, &[]);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Change \"libary\" to \"library\"");
        assert_eq!(actions[0].kind, Some(CodeActionKind::QUICKFIX));

        let edits = actions[0]
            .edit
            .as_ref()
            .and_then(|e| e.changes.as_ref())
            .and_then(|c| c.values().next())
            .expect("action should carry a text edit");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "library");
        assert_eq!(
            edits[0].range,
            Range::new(Position::new(0, 19), Position::new(0, 25))
        );
    }

    #[test]
    fn test_dep3_actions_filter_by_diagnostic() {
        let content = "Description: Fix a libary lookup bug\n---\n@@ -1 +1 @@\n";

        let unrelated = Diagnostic {
            range: Range::new(Position::new(2, 0), Position::new(2, 1)),
            code: Some(NumberOrString::String("spelling".to_string())),
            ..Default::default()
        };
        assert_eq!(dep3_acts(content, std::slice::from_ref(&unrelated)), vec![]);

        let matching = Diagnostic {
            range: Range::new(Position::new(0, 19), Position::new(0, 25)),
            code: Some(NumberOrString::String("spelling".to_string())),
            ..Default::default()
        };
        let actions = dep3_acts(content, std::slice::from_ref(&matching));
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].diagnostics, Some(vec![matching]));
    }
}
