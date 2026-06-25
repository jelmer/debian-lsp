//! Code actions and formatting shared by line-based debhelper config files.
//!
//! The quick fixes are keyed off the diagnostic `code` strings emitted by
//! [`crate::debhelper::diagnostics`], so the same handler serves every
//! debhelper helper. The wrap-and-sort formatter lives here too.

use crate::position::Source;
use tower_lsp_server::ls_types::*;

/// Generate code actions to fix diagnostics in a line-based debhelper file.
///
/// Handles:
/// - `duplicate-entry` -> delete the duplicate line
pub fn get_code_actions(
    src: Source<'_>,
    uri: &Uri,
    diagnostics: &[Diagnostic],
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();

    for diag in diagnostics {
        let code = match &diag.code {
            Some(NumberOrString::String(s)) => s.as_str(),
            _ => continue,
        };

        if code != "duplicate-entry" {
            continue;
        }

        let line_num = diag.range.start.line;
        let last_line = src.text.lines().count().saturating_sub(1);

        // Include the trailing newline so the line is deleted entirely.
        // Fall back to end-of-line range for the last line.
        let delete_range = if (line_num as usize) < last_line {
            Range {
                start: Position::new(line_num, 0),
                end: Position::new(line_num + 1, 0),
            }
        } else {
            diag.range
        };

        let workspace_edit = WorkspaceEdit {
            changes: Some(
                vec![(
                    uri.clone(),
                    vec![TextEdit {
                        range: delete_range,
                        new_text: String::new(),
                    }],
                )]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        };

        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: "Remove duplicate entry".to_string(),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![diag.clone()]),
            edit: Some(workspace_edit),
            ..Default::default()
        }));
    }

    actions
}

/// Format a line-based debhelper file the way `wrap-and-sort` does: trim each
/// line, drop blank lines, and sort what is left. Returns `None` when the
/// file is already formatted so the caller can skip the edit.
pub fn format_debhelper(source_text: &str) -> Option<String> {
    let mut lines: Vec<&str> = source_text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines.sort_unstable();

    let formatted = format!("{}\n", lines.join("\n"));
    if formatted == source_text {
        None
    } else {
        Some(formatted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;

    // The shared handler is file-type agnostic, so these tests use a neutral
    // URI and generic entries. The dirs- and install-specific cases live in
    // their own actions.rs.
    fn make_uri() -> Uri {
        "file:///tmp/debian/example".parse().unwrap()
    }

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
    fn test_duplicate_entry_action_deletes_line() {
        let text = "usr/bin\nusr/bin\n";
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let uri = make_uri();
        let diag = make_diagnostic("duplicate-entry", 1, 7);
        let actions = get_code_actions(src, &uri, &[diag]);
        assert!(!actions.is_empty());
        let CodeActionOrCommand::CodeAction(ref action) = actions[0] else {
            panic!("Expected CodeAction");
        };
        assert_eq!(action.title, "Remove duplicate entry");
        let changes = action.edit.as_ref().unwrap().changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits[0].new_text, "");
    }

    #[test]
    fn test_last_line_duplicate_uses_diagnostic_range() {
        // No trailing newline after the duplicate: fall back to its own range.
        let text = "usr/bin\nusr/bin";
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let uri = make_uri();
        let diag = make_diagnostic("duplicate-entry", 1, 7);
        let actions = get_code_actions(src, &uri, &[diag]);
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn test_unknown_code_produces_no_action() {
        let text = "usr/bin\n";
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let uri = make_uri();
        let diag = make_diagnostic("unknown", 0, 7);
        let actions = get_code_actions(src, &uri, &[diag]);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_wrap_and_sort_sorts_and_drops_blanks() {
        let text = "usr/share/myapp\n\nusr/bin\n";
        assert_eq!(
            format_debhelper(text).unwrap(),
            "usr/bin\nusr/share/myapp\n"
        );
    }

    #[test]
    fn test_wrap_and_sort_already_sorted_returns_none() {
        assert!(format_debhelper("usr/bin\nusr/share/myapp\n").is_none());
    }
}
