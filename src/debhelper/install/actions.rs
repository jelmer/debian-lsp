use crate::position::Source;
use tower_lsp_server::ls_types::{CodeActionOrCommand, Diagnostic, Uri};

/// Code actions for a debian/install file. The duplicate-entry quick fix is
/// shared by every debhelper helper; this is the install entry point.
pub fn get_code_actions(
    src: Source<'_>,
    uri: &Uri,
    diagnostics: &[Diagnostic],
) -> Vec<CodeActionOrCommand> {
    crate::debhelper::actions::get_code_actions(src, uri, diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;
    use tower_lsp_server::ls_types::{DiagnosticSeverity, NumberOrString, Position, Range};

    fn make_diagnostic(code: &str, line: u32, col_end: u32) -> Diagnostic {
        Diagnostic {
            range: Range {
                start: Position::new(line, 0),
                end: Position::new(line, col_end),
            },
            code: Some(NumberOrString::String(code.to_string())),
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("debian-lsp".to_string()),
            message: String::new(),
            ..Default::default()
        }
    }

    #[test]
    fn test_duplicate_install_entry_is_removed() {
        let text = "my-prog usr/bin\nmy-prog usr/bin\n";
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let uri: Uri = "file:///tmp/debian/install".parse().unwrap();
        let diag = make_diagnostic("duplicate-entry", 1, 15);
        let actions = get_code_actions(src, &uri, &[diag]);
        assert_eq!(actions.len(), 1);
        let CodeActionOrCommand::CodeAction(ref action) = actions[0] else {
            panic!("Expected CodeAction");
        };
        assert_eq!(action.title, "Remove duplicate entry");
    }

    #[test]
    fn test_unknown_code_produces_no_action() {
        let text = "my-prog usr/bin\n";
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let uri: Uri = "file:///tmp/debian/install".parse().unwrap();
        let diag = make_diagnostic("unknown", 0, 15);
        assert!(get_code_actions(src, &uri, &[diag]).is_empty());
    }
}
