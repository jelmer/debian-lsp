use crate::position::Source;
use crate::workspace::FieldCasingIssue;
use text_size::TextRange;
use tower_lsp_server::ls_types::*;

pub const ADD_BINARY_PACKAGE_COMMAND: &str = "debian-lsp.addBinaryPackage";

/// Format an entire control file using wrap-and-sort
///
/// # Arguments
/// * `src.text` - The source text of the file
/// * `parsed` - The parsed control file
///
/// # Returns
/// A list of text edits to apply, or None if the file is already formatted
pub fn format_control(
    src: Source<'_>,
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
) -> Option<Vec<TextEdit>> {
    let mut control = parsed.clone().to_result().ok()?;
    control.wrap_and_sort(deb822_lossless::Indentation::Spaces(1), false, Some(79));
    let formatted = control.to_string();
    if formatted == src.text {
        return None;
    }
    let full_range = src.text_range_to_lsp_range(text_size::TextRange::new(
        0.into(),
        (src.text.len() as u32).into(),
    ));
    Some(vec![TextEdit {
        range: full_range,
        new_text: formatted,
    }])
}

/// Generate a wrap-and-sort code action for a control file
///
/// This function creates a code action that wraps and sorts fields in paragraphs
/// that overlap with the requested text range.
///
/// # Arguments
/// * `uri` - The URI of the control file
/// * `src.text` - The source text of the file
/// * `parsed` - The parsed control file
/// * `text_range` - The text range to operate on
///
/// # Returns
/// A code action if applicable paragraphs are found, None otherwise
pub fn get_wrap_and_sort_action(
    uri: &Uri,
    src: Source<'_>,
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    text_range: TextRange,
) -> Option<CodeActionOrCommand> {
    let control = parsed.clone().to_result().ok()?;
    let mut edits = Vec::new();

    // Check if source paragraph is in range
    if let Some(source) = control.source_in_range(text_range) {
        let para_range = source.as_deb822().text_range();
        let mut source = source.clone();
        source.wrap_and_sort(deb822_lossless::Indentation::Spaces(1), false, Some(79));
        let lsp_range = src.text_range_to_lsp_range(para_range);
        edits.push(TextEdit {
            range: lsp_range,
            new_text: source.to_string(),
        });
    }

    // Check each binary paragraph in range
    for binary in control.binaries_in_range(text_range) {
        let para_range = binary.as_deb822().text_range();
        let mut binary = binary.clone();
        binary.wrap_and_sort(deb822_lossless::Indentation::Spaces(1), false, Some(79));
        let lsp_range = src.text_range_to_lsp_range(para_range);
        edits.push(TextEdit {
            range: lsp_range,
            new_text: binary.to_string(),
        });
    }

    if edits.is_empty() {
        return None;
    }

    let workspace_edit = WorkspaceEdit {
        changes: Some(vec![(uri.clone(), edits)].into_iter().collect()),
        ..Default::default()
    };

    let action = CodeAction {
        title: "Wrap and sort".to_string(),
        kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
        edit: Some(workspace_edit),
        ..Default::default()
    };

    Some(CodeActionOrCommand::CodeAction(action))
}

/// Build the workspace edit that appends a new binary package stanza.
///
/// Used both by the command handler (`execute_command`) and tests.
pub fn build_add_binary_package_edit(
    uri: &Uri,
    src: Source<'_>,
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
) -> Option<WorkspaceEdit> {
    let control = parsed.clone().to_result().ok()?;
    let source = control.source()?;
    let source_name = source.name()?;

    let mut new_control = debian_control::lossless::Control::new();
    let mut binary = new_control.add_binary(&source_name);
    binary
        .as_mut_deb822()
        .set("Depends", "${shlibs:Depends}, ${misc:Depends}");
    binary.set_description(Some(&format!("<insert description for {}>", source_name)));

    let binary_text = new_control
        .binaries()
        .next()
        .unwrap()
        .as_deb822()
        .to_string();

    let end_offset = src.text.len() as u32;
    let end_position = src.offset_to_position(end_offset.into());

    Some(WorkspaceEdit {
        changes: Some(
            vec![(
                uri.clone(),
                vec![TextEdit {
                    range: Range {
                        start: end_position,
                        end: end_position,
                    },
                    new_text: format!("\n{}", binary_text),
                }],
            )]
            .into_iter()
            .collect(),
        ),
        ..Default::default()
    })
}

/// Return a palette command entry for "Add binary package".
///
/// This is intentionally a `Command` (not a `CodeAction`) so that VS Code
/// only surfaces it via the command palette, not the automatic lightbulb.
pub fn get_add_binary_package_command(uri: &Uri) -> CodeActionOrCommand {
    CodeActionOrCommand::Command(Command {
        title: "Add binary package".to_string(),
        command: ADD_BINARY_PACKAGE_COMMAND.to_string(),
        arguments: Some(vec![serde_json::json!(uri.as_str())]),
    })
}

/// Generate field casing fix actions for a control file
///
/// # Arguments
/// * `uri` - The URI of the control file
/// * `src.text` - The source text of the file
/// * `issues` - The field casing issues found
/// * `diagnostics` - The diagnostics from the context
///
/// # Returns
/// A vector of code actions for fixing field casing
pub fn get_field_casing_actions(
    uri: &Uri,
    src: Source<'_>,
    issues: Vec<FieldCasingIssue>,
    diagnostics: &[Diagnostic],
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();

    for issue in issues {
        let lsp_range = src.text_range_to_lsp_range(issue.field_range);

        let matching_diagnostics = diagnostics
            .iter()
            .filter(|d| {
                d.range == lsp_range
                    && d.code == Some(NumberOrString::String("field-casing".to_string()))
            })
            .cloned()
            .collect::<Vec<_>>();

        // When the client specified diagnostics it wants fixes for, only
        // emit actions that match one of them — otherwise VS Code shows
        // this quickfix for every squiggle in the vicinity.
        if !diagnostics.is_empty() && matching_diagnostics.is_empty() {
            continue;
        }

        let edit = TextEdit {
            range: lsp_range,
            new_text: issue.standard_name.clone(),
        };

        let workspace_edit = WorkspaceEdit {
            changes: Some(vec![(uri.clone(), vec![edit])].into_iter().collect()),
            ..Default::default()
        };

        let action = CodeAction {
            title: format!(
                "Fix field casing: {} -> {}",
                issue.field_name, issue.standard_name
            ),
            kind: Some(CodeActionKind::QUICKFIX),
            edit: Some(workspace_edit),
            diagnostics: if !matching_diagnostics.is_empty() {
                Some(matching_diagnostics)
            } else {
                None
            },
            ..Default::default()
        };

        actions.push(CodeActionOrCommand::CodeAction(action));
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_and_sort_action() {
        let input = r#"Source: test-package
Maintainer: Test User <test@example.com>
Build-Depends: debhelper-compat (= 13), foo, bar, baz

Package: test-package
Architecture: any
Depends: libc6, libfoo, libbar
Description: A test package
 This is a test package.
"#;

        let parsed = debian_control::lossless::Control::parse(input);
        let idx = crate::position::LineIndex::new(input);
        let src = Source::new(input, &idx);
        let uri: Uri = "file:///debian/control".parse().unwrap();
        let text_range = TextRange::new(0.into(), (input.len() as u32).into());

        let action = get_wrap_and_sort_action(&uri, src, &parsed, text_range);

        // Should return a code action
        assert!(action.is_some());

        let CodeActionOrCommand::CodeAction(action) = action.unwrap() else {
            panic!("Expected CodeAction");
        };

        assert_eq!(action.title, "Wrap and sort");
        assert_eq!(action.kind, Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS));

        // Extract the edits
        let workspace_edit = action.edit.expect("Should have an edit");
        let changes = workspace_edit.changes.expect("Should have changes");
        let edits = changes.get(&uri).expect("Should have edits for the URI");

        // Should have edits for both source and binary paragraphs
        assert_eq!(edits.len(), 2);

        // Get the formatted source paragraph
        let formatted_source = &edits[0].new_text;

        // Get the formatted binary paragraph
        let formatted_binary = &edits[1].new_text;

        // Print the actual output for debugging
        println!("Formatted source:\n{}", formatted_source);
        println!("Formatted binary:\n{}", formatted_binary);

        // Verify the exact formatted output for source paragraph
        let expected_source = "Source: test-package\nMaintainer: Test User <test@example.com>\nBuild-Depends:bar, baz, debhelper-compat (= 13), foo\n";
        assert_eq!(formatted_source, expected_source);

        // Verify the exact formatted output for binary paragraph
        let expected_binary = "Package: test-package\nArchitecture: any\nDepends:libbar, libc6, libfoo\nDescription: A test package\n This is a test package.\n";
        assert_eq!(formatted_binary, expected_binary);
    }

    #[test]
    fn test_field_casing_actions() {
        let input = "source: test-package\nmaintainer: Test User <test@example.com>\n";

        let idx = crate::position::LineIndex::new(input);
        let src = Source::new(input, &idx);
        let uri: Uri = "file:///debian/control".parse().unwrap();
        let issues = vec![
            FieldCasingIssue {
                field_name: "source".to_string(),
                standard_name: "Source".to_string(),
                field_range: TextRange::new(0.into(), 6.into()),
            },
            FieldCasingIssue {
                field_name: "maintainer".to_string(),
                standard_name: "Maintainer".to_string(),
                field_range: TextRange::new(21.into(), 31.into()),
            },
        ];

        // Empty diagnostics: all issues in range are returned.
        let actions = get_field_casing_actions(&uri, src, issues.clone(), &[]);
        assert_eq!(actions.len(), 2);

        let CodeActionOrCommand::CodeAction(ref action) = actions[0] else {
            panic!("Expected CodeAction");
        };
        assert_eq!(action.title, "Fix field casing: source -> Source");

        let CodeActionOrCommand::CodeAction(ref action) = actions[1] else {
            panic!("Expected CodeAction");
        };
        assert_eq!(action.title, "Fix field casing: maintainer -> Maintainer");
    }

    #[test]
    fn test_field_casing_actions_filters_to_context_diagnostics() {
        // Two casing issues, but context only contains a diagnostic for the
        // first one.  The server must return only the matching action and link
        // the diagnostic object to it.
        let input = "source: test-package\nmaintainer: Test User <test@example.com>\n";

        let idx = crate::position::LineIndex::new(input);
        let src = Source::new(input, &idx);
        let uri: Uri = "file:///debian/control".parse().unwrap();
        let issues = vec![
            FieldCasingIssue {
                field_name: "source".to_string(),
                standard_name: "Source".to_string(),
                field_range: TextRange::new(0.into(), 6.into()),
            },
            FieldCasingIssue {
                field_name: "maintainer".to_string(),
                standard_name: "Maintainer".to_string(),
                field_range: TextRange::new(21.into(), 31.into()),
            },
        ];

        // Only the "source" diagnostic is in context.
        let source_diag = Diagnostic {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 6,
                },
            },
            code: Some(NumberOrString::String("field-casing".to_string())),
            message: "Field name should be 'Source'".to_string(),
            ..Default::default()
        };
        let actions = get_field_casing_actions(&uri, src, issues, &[source_diag.clone()]);

        // Only the "source" action is returned.
        assert_eq!(actions.len(), 1);
        let CodeActionOrCommand::CodeAction(ref action) = actions[0] else {
            panic!("Expected CodeAction");
        };
        assert_eq!(action.title, "Fix field casing: source -> Source");
        // The action is linked back to the provided diagnostic.
        assert_eq!(action.diagnostics, Some(vec![source_diag]));
    }

    #[test]
    fn test_add_binary_package_action() {
        let input = "Source: my-package\nSection: utils\nPriority: optional\nMaintainer: Test User <test@example.com>\n";

        let parsed = debian_control::lossless::Control::parse(input);
        let idx = crate::position::LineIndex::new(input);
        let src = Source::new(input, &idx);
        let uri: Uri = "file:///debian/control".parse().unwrap();

        let edit = build_add_binary_package_edit(&uri, src, &parsed);
        assert!(edit.is_some());

        let changes = edit.unwrap().changes.expect("Should have changes");
        let edits = changes.get(&uri).expect("Should have edits for the URI");
        assert_eq!(edits.len(), 1);

        // Should be inserted at the end of the file
        let end_line = input.lines().count() as u32;
        assert_eq!(edits[0].range.start.line, end_line);

        assert_eq!(
            edits[0].new_text,
            "\nPackage: my-package\nDepends: ${shlibs:Depends}, ${misc:Depends}\nDescription: <insert description for my-package>\n"
        );
    }

    #[test]
    fn test_format_control() {
        let input = "Source: test-package\nMaintainer: Test User <test@example.com>\nBuild-Depends: debhelper-compat (= 13), foo, bar, baz\n\nPackage: test-package\nArchitecture: any\nDepends: libc6, libfoo, libbar\nDescription: A test package\n This is a test package.\n";

        let parsed = debian_control::lossless::Control::parse(input);
        let idx = crate::position::LineIndex::new(input);
        let src = Source::new(input, &idx);
        let edits = format_control(src, &parsed);

        assert!(edits.is_some());
        let edits = edits.unwrap();
        assert_eq!(edits.len(), 1);

        // The single edit should cover the entire document
        assert_eq!(edits[0].range.start.line, 0);
        assert_eq!(edits[0].range.start.character, 0);

        // Verify the formatted output is different from input
        assert_ne!(edits[0].new_text, input);
    }

    #[test]
    fn test_format_control_already_formatted() {
        // Create a file, format it, then verify formatting again returns None
        let input = "Source: test-package\nMaintainer: Test User <test@example.com>\n";

        let parsed = debian_control::lossless::Control::parse(input);
        let idx = crate::position::LineIndex::new(input);
        let first_format = format_control(Source::new(input, &idx), &parsed);

        // Apply the first format (or use original if already formatted)
        let formatted = match first_format {
            Some(edits) => edits[0].new_text.clone(),
            None => input.to_string(),
        };

        // Format again - should return None since already formatted
        let parsed2 = debian_control::lossless::Control::parse(&formatted);
        let idx2 = crate::position::LineIndex::new(&formatted);
        let second_format = format_control(Source::new(&formatted, &idx2), &parsed2);
        assert!(second_format.is_none());
    }
}
