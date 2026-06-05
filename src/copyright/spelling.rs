//! Spell-checking for prose fields in `debian/copyright`.
//!
//! The generic deb822 checker lives in [`crate::spelling::deb822`]; this module
//! supplies the copyright-specific wiring: which fields hold free-form prose.
//! Only `Comment` and `Disclaimer` are checked. License text bodies, file
//! globs, copyright statements and SPDX expressions are left alone to avoid
//! false positives on quoted legal text and identifiers.

use tower_lsp_server::ls_types::{CodeActionOrCommand, Diagnostic, Uri};

use crate::position::Source;
use crate::spelling::deb822::deb822_findings;
use crate::spelling::{make_actions, make_diagnostic, LocatedFinding};

/// Field names in `debian/copyright` whose values are free-form prose worth
/// spell-checking.
fn is_prose_field(field_name: &str) -> bool {
    field_name.eq_ignore_ascii_case("Comment") || field_name.eq_ignore_ascii_case("Disclaimer")
}

/// Find all spelling mistakes in the prose fields of a `debian/copyright` file,
/// each mapped to its source range.
fn copyright_findings(
    parsed: &debian_copyright::lossless::Parse,
    src: Source<'_>,
) -> Vec<LocatedFinding> {
    deb822_findings(parsed.tree().as_deb822(), src, is_prose_field)
}

/// Produce spelling diagnostics for the prose fields of a `debian/copyright`
/// file.
pub fn copyright_diagnostics(
    parsed: &debian_copyright::lossless::Parse,
    src: Source<'_>,
) -> Vec<Diagnostic> {
    copyright_findings(parsed, src)
        .into_iter()
        .map(|located| make_diagnostic(located.range, &located.finding))
        .collect()
}

/// Produce quick-fix code actions for spelling mistakes in a `debian/copyright`
/// file. One action per suggested correction.
///
/// When `diagnostics` is non-empty (the client requested fixes for specific
/// diagnostics), only actions whose range matches one of them are emitted, so
/// the quickfix doesn't appear for every nearby squiggle.
pub fn copyright_actions(
    uri: &Uri,
    parsed: &debian_copyright::lossless::Parse,
    src: Source<'_>,
    diagnostics: &[Diagnostic],
) -> Vec<CodeActionOrCommand> {
    make_actions(uri, copyright_findings(parsed, src), diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;
    use tower_lsp_server::ls_types::{CodeAction, CodeActionKind, NumberOrString, Position, Range};

    fn copyright_diags(content: &str) -> Vec<Diagnostic> {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/copyright").unwrap();
        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_copyright(file);
        let text = workspace.source_text(file);
        let idx = workspace.get_line_index(file);
        let src = Source::new(&text, &idx);
        copyright_diagnostics(&parsed, src)
    }

    #[test]
    fn test_copyright_diagnostics_comment_typo() {
        let content = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nComment: This is teh upstream source.\n";
        let diags = copyright_diags(content);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "\"teh\" should be \"the\"");
        // "teh" starts at column 17 on the Comment line (line 1).
        assert_eq!(
            diags[0].range,
            Range::new(Position::new(1, 17), Position::new(1, 20))
        );
    }

    #[test]
    fn test_copyright_diagnostics_disclaimer_typo() {
        let content = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: *\nCopyright: 2024 Foo\nLicense: MIT\nDisclaimer: This packge is provided as-is.\n";
        let diags = copyright_diags(content);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "\"packge\" should be \"package\"");
        assert_eq!(diags[0].range.start.line, 5);
    }

    #[test]
    fn test_copyright_diagnostics_skips_license_and_copyright() {
        // A typo-shaped token in License/Copyright/Files must not be flagged.
        let content = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: libary/*\nCopyright: 2024 teh Author\nLicense: GPL-2+\n This libary is free software.\n";
        assert_eq!(copyright_diags(content), vec![]);
    }

    #[test]
    fn test_copyright_diagnostics_multiline_comment() {
        // Typo on a continuation line: the joined-value offset must map back
        // through the '\n' separator to the right source position.
        let content = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nComment: short\n A longer note that recieves input.\n";
        let diags = copyright_diags(content);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "\"recieves\" should be \"receives\"");
        assert_eq!(diags[0].range.start.line, 2);
    }

    fn copyright_acts(content: &str, diagnostics: &[Diagnostic]) -> Vec<CodeAction> {
        let mut workspace = Workspace::new();
        let url: Uri = str::parse("file:///debian/copyright").unwrap();
        let file = workspace.update_file(url.clone(), content.to_string());
        let parsed = workspace.get_parsed_copyright(file);
        let text = workspace.source_text(file);
        let idx = workspace.get_line_index(file);
        let src = Source::new(&text, &idx);
        copyright_actions(&url, &parsed, src, diagnostics)
            .into_iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(action) => action,
                CodeActionOrCommand::Command(_) => panic!("expected CodeAction"),
            })
            .collect()
    }

    #[test]
    fn test_copyright_actions_quickfix() {
        let content = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nComment: This is teh upstream source.\n";
        let actions = copyright_acts(content, &[]);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Change \"teh\" to \"the\"");
        assert_eq!(actions[0].kind, Some(CodeActionKind::QUICKFIX));

        let edits = actions[0]
            .edit
            .as_ref()
            .and_then(|e| e.changes.as_ref())
            .and_then(|c| c.values().next())
            .expect("action should carry a text edit");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "the");
        assert_eq!(
            edits[0].range,
            Range::new(Position::new(1, 17), Position::new(1, 20))
        );
    }

    #[test]
    fn test_copyright_actions_filter_by_diagnostic() {
        let content = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nComment: This is teh upstream source.\n";

        let unrelated = Diagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 1)),
            code: Some(NumberOrString::String("spelling".to_string())),
            ..Default::default()
        };
        assert_eq!(
            copyright_acts(content, std::slice::from_ref(&unrelated)),
            vec![]
        );

        let matching = Diagnostic {
            range: Range::new(Position::new(1, 17), Position::new(1, 20)),
            code: Some(NumberOrString::String("spelling".to_string())),
            ..Default::default()
        };
        let actions = copyright_acts(content, std::slice::from_ref(&matching));
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].diagnostics, Some(vec![matching]));
    }
}
