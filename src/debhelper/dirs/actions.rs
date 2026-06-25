use crate::position::Source;
use tower_lsp_server::ls_types::*;

/// Generate code actions to fix diagnostic issues in a debian/dirs file.
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

/// Format a debian/dirs file.
pub fn format_dirs(source_text: &str) -> Option<String> {
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

    fn make_uri() -> Uri {
        "file:///tmp/debian/dirs".parse().unwrap()
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
        let text = "usr/share/myapp\nusr/share/myapp\n";
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let uri = make_uri();
        let diag = make_diagnostic("duplicate-entry", 1, 15);
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
    fn test_unknown_code_produces_no_action() {
        let text = "usr/share/myapp\n";
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let uri = make_uri();
        let diag = make_diagnostic("unknown", 0, 15);
        let actions = get_code_actions(src, &uri, &[diag]);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_format_sorts_entries_alphabetically() {
        let text = "usr/share/myapp\netc/myapp\nusr/bin\n";
        let formatted = format_dirs(text).unwrap();
        assert_eq!(formatted, "etc/myapp\nusr/bin\nusr/share/myapp\n");
    }

    #[test]
    fn test_format_drops_blank_lines() {
        let text = "usr/bin\n\netc/myapp\n\n";
        let formatted = format_dirs(text).unwrap();
        assert_eq!(formatted, "etc/myapp\nusr/bin\n");
    }

    #[test]
    fn test_format_strips_leading_and_trailing_whitespace() {
        let text = "  usr/bin  \n\tetc/myapp\t\n";
        let formatted = format_dirs(text).unwrap();
        assert_eq!(formatted, "etc/myapp\nusr/bin\n");
    }

    #[test]
    fn test_format_comments_sorted_in_with_entries() {
        let text = "usr/bin\n# a comment\netc/myapp\n";
        let formatted = format_dirs(text).unwrap();
        assert_eq!(formatted, "# a comment\netc/myapp\nusr/bin\n");
    }

    #[test]
    fn test_format_already_sorted_returns_none() {
        let text = "etc/myapp\nusr/bin\n";
        assert!(format_dirs(text).is_none());
    }

    #[test]
    fn test_format_empty_file_normalizes_to_single_newline() {
        let formatted = format_dirs("").unwrap();
        assert_eq!(formatted, "\n");
    }

    #[test]
    fn test_format_only_blank_lines_normalizes_to_single_newline() {
        let formatted = format_dirs("\n\n\n").unwrap();
        assert_eq!(formatted, "\n");
    }

    #[test]
    fn test_format_missing_trailing_newline_gets_added() {
        let text = "usr/bin";
        let formatted = format_dirs(text).unwrap();
        assert_eq!(formatted, "usr/bin\n");
    }
}
