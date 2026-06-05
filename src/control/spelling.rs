//! Spell-checking for prose fields in `debian/control`.
//!
//! The generic checker lives in [`crate::spelling`]; this module supplies the
//! control-specific wiring: which fields hold free-form prose worth checking,
//! and how to map typo offsets in a joined field value back to source ranges.
//! Only free-text field values (e.g. `Description`) are checked, never field
//! names, package relationships or URLs.

use tower_lsp_server::ls_types::{CodeActionOrCommand, Diagnostic, Uri};

use crate::position::Source;
use crate::spelling::deb822::deb822_findings;
use crate::spelling::{make_actions, make_diagnostic, LocatedFinding};

/// Field names in `debian/control` whose values are free-form prose worth
/// spell-checking. Everything else (relationships, URLs, package names,
/// architectures) is skipped to avoid false positives.
fn is_prose_field(field_name: &str) -> bool {
    field_name.eq_ignore_ascii_case("Description")
}

/// Find all spelling mistakes in the prose fields of a `debian/control` file,
/// each mapped to its source range.
fn control_findings(
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    src: Source<'_>,
) -> Vec<LocatedFinding> {
    deb822_findings(parsed.tree().as_deb822(), src, is_prose_field)
}

/// Produce spelling diagnostics for the prose fields of a `debian/control`
/// file.
pub fn control_diagnostics(
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    src: Source<'_>,
) -> Vec<Diagnostic> {
    control_findings(parsed, src)
        .into_iter()
        .map(|located| make_diagnostic(located.range, &located.finding))
        .collect()
}

/// Produce quick-fix code actions for spelling mistakes in a `debian/control`
/// file. One action per suggested correction.
///
/// When `diagnostics` is non-empty (the client requested fixes for specific
/// diagnostics), only actions whose range matches one of them are emitted, so
/// the quickfix doesn't appear for every nearby squiggle.
pub fn control_actions(
    uri: &Uri,
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    src: Source<'_>,
    diagnostics: &[Diagnostic],
) -> Vec<CodeActionOrCommand> {
    make_actions(uri, control_findings(parsed, src), diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spelling::is_spelling_diagnostic;
    use crate::workspace::Workspace;
    use tower_lsp_server::ls_types::{CodeAction, CodeActionKind, NumberOrString, Position, Range};

    fn control_diags(content: &str) -> Vec<Diagnostic> {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/control").unwrap();
        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_control(file);
        let text = workspace.source_text(file);
        let idx = workspace.get_line_index(file);
        let src = Source::new(&text, &idx);
        control_diagnostics(&parsed, src)
    }

    #[test]
    fn test_control_diagnostics_description_typo() {
        let content =
            "Source: foo\n\nPackage: foo\nArchitecture: all\nDescription: a libary for things\n";
        let diags = control_diags(content);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "\"libary\" should be \"library\"");
        // "libary" starts at column 15 on the Description line (line 4).
        assert_eq!(
            diags[0].range,
            Range::new(Position::new(4, 15), Position::new(4, 21))
        );
    }

    #[test]
    fn test_control_diagnostics_skips_non_prose_fields() {
        // A typo-shaped token in a non-prose field must not be flagged.
        let content = "Source: foo\nBuild-Depends: libary-dev\n\nPackage: foo\nArchitecture: all\nDescription: clean text\n";
        assert_eq!(control_diags(content), vec![]);
    }

    #[test]
    fn test_control_diagnostics_multiline_description() {
        // Typo on a continuation line: the joined-value offset must map back
        // through the '\n' separator to the right source position.
        let content =
            "Source: foo\n\nPackage: foo\nArchitecture: all\nDescription: short\n A longer paragraph that recieves input.\n";
        let diags = control_diags(content);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "\"recieves\" should be \"receives\"");
        assert_eq!(diags[0].range.start.line, 5);
    }

    fn control_acts(content: &str, diagnostics: &[Diagnostic]) -> Vec<CodeAction> {
        let mut workspace = Workspace::new();
        let url: Uri = str::parse("file:///debian/control").unwrap();
        let file = workspace.update_file(url.clone(), content.to_string());
        let parsed = workspace.get_parsed_control(file);
        let text = workspace.source_text(file);
        let idx = workspace.get_line_index(file);
        let src = Source::new(&text, &idx);
        control_actions(&url, &parsed, src, diagnostics)
            .into_iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(action) => action,
                CodeActionOrCommand::Command(_) => panic!("expected CodeAction"),
            })
            .collect()
    }

    #[test]
    fn test_control_actions_quickfix() {
        let content =
            "Source: foo\n\nPackage: foo\nArchitecture: all\nDescription: a libary for things\n";
        // No diagnostics filter: every finding yields an action.
        let actions = control_acts(content, &[]);
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
            Range::new(Position::new(4, 15), Position::new(4, 21))
        );
    }

    #[test]
    fn test_control_actions_filter_by_diagnostic() {
        let content =
            "Source: foo\n\nPackage: foo\nArchitecture: all\nDescription: a libary for things\n";

        // A diagnostics filter that matches nothing suppresses all actions.
        let unrelated = Diagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 1)),
            code: Some(NumberOrString::String("spelling".to_string())),
            ..Default::default()
        };
        assert_eq!(
            control_acts(content, std::slice::from_ref(&unrelated)),
            vec![]
        );

        // A filter that matches the finding's range yields the action, with
        // the originating diagnostic attached.
        let matching = Diagnostic {
            range: Range::new(Position::new(4, 15), Position::new(4, 21)),
            code: Some(NumberOrString::String("spelling".to_string())),
            ..Default::default()
        };
        let actions = control_acts(content, std::slice::from_ref(&matching));
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].diagnostics, Some(vec![matching]));
    }

    #[test]
    fn test_is_spelling_diagnostic() {
        let spelling = Diagnostic {
            code: Some(NumberOrString::String("spelling".to_string())),
            ..Default::default()
        };
        assert!(is_spelling_diagnostic(&spelling));
        assert!(!is_spelling_diagnostic(&Diagnostic::default()));
    }
}
