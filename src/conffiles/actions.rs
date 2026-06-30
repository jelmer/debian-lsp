use crate::position::Source;
use tower_lsp_server::ls_types::*;

/// Generate code actions to fix diagnostic issues in a debian/conffiles file.
///
/// Handles:
/// - `empty-line`      -> delete the line
/// - `relative-path`   -> prepend `/` to make the path absolute
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

        match code {
            "empty-line" | "duplicate-entry" => {
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

                let title = if code == "empty-line" {
                    "Remove empty line"
                } else {
                    "Remove duplicate entry"
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
                    title: title.to_string(),
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![diag.clone()]),
                    edit: Some(workspace_edit),
                    ..Default::default()
                }));
            }

            "relative-path" => {
                let line_num = diag.range.start.line;
                let line = src.text.lines().nth(line_num as usize).unwrap_or("");

                // If the line starts with the flag, insert '/' after the flag and space
                let flag = "remove-on-upgrade ";
                let insert_col = if line.starts_with(flag) {
                    flag.len() as u32
                } else {
                    0
                };

                let insert_pos = Position::new(line_num, insert_col);
                let insert_range = Range::new(insert_pos, insert_pos);

                let workspace_edit = WorkspaceEdit {
                    changes: Some(
                        vec![(
                            uri.clone(),
                            vec![TextEdit {
                                range: insert_range,
                                new_text: "/".to_string(),
                            }],
                        )]
                        .into_iter()
                        .collect(),
                    ),
                    ..Default::default()
                };

                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: "Prepend '/' to make path absolute".to_string(),
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![diag.clone()]),
                    edit: Some(workspace_edit),
                    ..Default::default()
                }));
            }

            "unknown-flag" | "too-many-tokens" => {}
            _ => {}
        }
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;

    fn make_uri() -> Uri {
        "file:///tmp/debian/conffiles".parse().unwrap()
    }

    fn make_diagnostic(code: &str, line: u32, col_end: u32) -> Diagnostic {
        Diagnostic {
            range: Range {
                start: Position::new(line, 0),
                end: Position::new(line, col_end),
            },
            code: Some(NumberOrString::String(code.to_string())),
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("debian-lsp".to_string()),
            message: String::new(),
            ..Default::default()
        }
    }

    #[test]
    fn test_empty_line_action_deletes_line() {
        let text = "\n/etc/foo\n";
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let uri = make_uri();
        let diag = make_diagnostic("empty-line", 0, 0);

        let actions = get_code_actions(src, &uri, &[diag]);
        assert_eq!(actions.len(), 1);

        let CodeActionOrCommand::CodeAction(ref action) = actions[0] else {
            panic!("Expected CodeAction");
        };
        assert_eq!(action.title, "Remove empty line");

        let changes = action.edit.as_ref().unwrap().changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "");
        assert_eq!(edits[0].range.end.line, 1);
        assert_eq!(edits[0].range.end.character, 0);
    }

    #[test]
    fn test_relative_path_action_prepends_slash() {
        let text = "etc/foo\n";
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let uri = make_uri();
        let diag = make_diagnostic("relative-path", 0, 7);

        let actions = get_code_actions(src, &uri, &[diag]);
        assert_eq!(actions.len(), 1);

        let CodeActionOrCommand::CodeAction(ref action) = actions[0] else {
            panic!("Expected CodeAction");
        };
        assert_eq!(action.title, "Prepend '/' to make path absolute");

        let changes = action.edit.as_ref().unwrap().changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "/");
        assert_eq!(edits[0].range.start, Position::new(0, 0));
        assert_eq!(edits[0].range.end, Position::new(0, 0));
    }

    #[test]
    fn test_duplicate_entry_action_deletes_line() {
        let text = "/etc/foo\n/etc/foo\n";
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let uri = make_uri();
        let diag = make_diagnostic("duplicate-entry", 1, 8);

        let actions = get_code_actions(src, &uri, &[diag]);
        assert_eq!(actions.len(), 1);

        let CodeActionOrCommand::CodeAction(ref action) = actions[0] else {
            panic!("Expected CodeAction");
        };
        assert_eq!(action.title, "Remove duplicate entry");
    }

    #[test]
    fn test_unknown_code_produces_no_action() {
        let text = "/etc/foo\n";
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let uri = make_uri();
        let diag = make_diagnostic("unknown-flag", 0, 8);

        let actions = get_code_actions(src, &uri, &[diag]);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_relative_path_after_flag_inserts_at_correct_col() {
        let text = "remove-on-upgrade etc/foo\n";
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let uri = make_uri();
        let diag = make_diagnostic("relative-path", 0, 25);

        let actions = get_code_actions(src, &uri, &[diag]);
        assert_eq!(actions.len(), 1);

        let CodeActionOrCommand::CodeAction(ref action) = actions[0] else {
            panic!("Expected CodeAction");
        };
        let changes = action.edit.as_ref().unwrap().changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits[0].range.start.character, 18);
    }
}
