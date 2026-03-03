use crate::position::text_range_to_lsp_range;
use crate::workspace::FieldCasingIssue;
use text_size::TextRange;
use tower_lsp_server::ls_types::*;

/// Generate a wrap-and-sort code action for a control file
///
/// This function creates a code action that wraps and sorts fields in paragraphs
/// that overlap with the requested text range.
///
/// # Arguments
/// * `uri` - The URI of the control file
/// * `source_text` - The source text of the file
/// * `parsed` - The parsed control file
/// * `text_range` - The text range to operate on
///
/// # Returns
/// A code action if applicable paragraphs are found, None otherwise
pub fn get_wrap_and_sort_action(
    uri: &Uri,
    source_text: &str,
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
        let lsp_range = text_range_to_lsp_range(source_text, para_range);
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
        let lsp_range = text_range_to_lsp_range(source_text, para_range);
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

/// Generate field casing fix actions for a control file
///
/// # Arguments
/// * `uri` - The URI of the control file
/// * `source_text` - The source text of the file
/// * `issues` - The field casing issues found
/// * `diagnostics` - The diagnostics from the context
///
/// # Returns
/// A vector of code actions for fixing field casing
pub fn get_field_casing_actions(
    uri: &Uri,
    source_text: &str,
    issues: Vec<FieldCasingIssue>,
    diagnostics: &[Diagnostic],
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();

    for issue in issues {
        let lsp_range = text_range_to_lsp_range(source_text, issue.field_range);

        // Check if there's a matching diagnostic in the context
        let matching_diagnostics = diagnostics
            .iter()
            .filter(|d| {
                d.range == lsp_range
                    && d.code == Some(NumberOrString::String("field-casing".to_string()))
            })
            .cloned()
            .collect::<Vec<_>>();

        // Create a code action to fix the casing
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
        let uri: Uri = "file:///debian/control".parse().unwrap();
        let text_range = TextRange::new(0.into(), (input.len() as u32).into());

        let action = get_wrap_and_sort_action(&uri, input, &parsed, text_range);

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
        let input = r#"source: test-package
maintainer: Test User <test@example.com>
"#;

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

        let actions = get_field_casing_actions(&uri, input, issues, &[]);

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
}
