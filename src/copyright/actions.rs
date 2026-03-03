use crate::position::text_range_to_lsp_range;
use crate::workspace::FieldCasingIssue;
use text_size::TextRange;
use tower_lsp_server::ls_types::*;

/// Generate a wrap-and-sort code action for a copyright file
///
/// This function creates a code action that wraps and sorts fields in paragraphs
/// that overlap with the requested text range.
///
/// # Arguments
/// * `uri` - The URI of the copyright file
/// * `source_text` - The source text of the file
/// * `parsed` - The parsed copyright file
/// * `text_range` - The text range to operate on
///
/// # Returns
/// A code action if applicable paragraphs are found, None otherwise
pub fn get_wrap_and_sort_action(
    uri: &Uri,
    source_text: &str,
    parsed: &debian_copyright::lossless::Parse,
    text_range: TextRange,
) -> Option<CodeActionOrCommand> {
    let copyright = parsed.clone().to_result().ok()?;
    let mut edits = Vec::new();

    // Check if header paragraph is in range
    if let Some(header) = copyright.header_in_range(text_range) {
        let para_range = header.as_deb822().text_range();
        let formatted_para = header.as_deb822().wrap_and_sort(
            deb822_lossless::Indentation::Spaces(1),
            false,
            Some(79),
            None,
            None,
        );
        let lsp_range = text_range_to_lsp_range(source_text, para_range);
        edits.push(TextEdit {
            range: lsp_range,
            new_text: formatted_para.to_string(),
        });
    }

    // Check each files paragraph in range
    for files in copyright.iter_files_in_range(text_range) {
        let para_range = files.as_deb822().text_range();
        let formatted_para = files.as_deb822().wrap_and_sort(
            deb822_lossless::Indentation::Spaces(1),
            false,
            Some(79),
            None,
            None,
        );
        let lsp_range = text_range_to_lsp_range(source_text, para_range);
        edits.push(TextEdit {
            range: lsp_range,
            new_text: formatted_para.to_string(),
        });
    }

    // Check each license paragraph in range
    for license_para in copyright.iter_licenses_in_range(text_range) {
        let para_range = license_para.as_deb822().text_range();
        let formatted_para = license_para.as_deb822().wrap_and_sort(
            deb822_lossless::Indentation::Spaces(1),
            false,
            Some(79),
            None,
            None,
        );
        let lsp_range = text_range_to_lsp_range(source_text, para_range);
        edits.push(TextEdit {
            range: lsp_range,
            new_text: formatted_para.to_string(),
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

/// Generate field casing fix actions for a copyright file
///
/// # Arguments
/// * `uri` - The URI of the copyright file
/// * `source_text` - The source text
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
        let input = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: test-package
Source: https://example.com/test

Files: *
Copyright: 2024 Test User <test@example.com>
License: GPL-3+

License: GPL-3+
 This program is free software.
"#;

        let parsed = debian_copyright::lossless::Parse::parse(input);
        let uri: Uri = "file:///debian/copyright".parse().unwrap();
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

        // Should have edits for header, files, and license paragraphs
        assert_eq!(edits.len(), 3);

        // Verify the actual formatted output
        let formatted_header = &edits[0].new_text;
        let formatted_files = &edits[1].new_text;
        let formatted_license = &edits[2].new_text;

        println!("Formatted header:\n{}", formatted_header);
        println!("Formatted files:\n{}", formatted_files);
        println!("Formatted license:\n{}", formatted_license);

        // Check that header is properly formatted (fields sorted alphabetically)
        assert_eq!(
            formatted_header,
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nUpstream-Name: test-package\nSource: https://example.com/test\n"
        );

        // Check that files paragraph is properly formatted
        assert_eq!(
            formatted_files,
            "Files: *\nCopyright: 2024 Test User <test@example.com>\nLicense: GPL-3+\n"
        );

        // Check that license paragraph is properly formatted
        assert_eq!(
            formatted_license,
            "License: GPL-3+\n This program is free software.\n"
        );
    }

    #[test]
    fn test_field_casing_actions() {
        let input = r#"format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
upstream-name: test
"#;

        let uri: Uri = "file:///debian/copyright".parse().unwrap();
        let issues = vec![
            FieldCasingIssue {
                field_name: "format".to_string(),
                standard_name: "Format".to_string(),
                field_range: TextRange::new(0.into(), 6.into()),
            },
            FieldCasingIssue {
                field_name: "upstream-name".to_string(),
                standard_name: "Upstream-Name".to_string(),
                field_range: TextRange::new(76.into(), 89.into()),
            },
        ];

        let actions = get_field_casing_actions(&uri, input, issues, &[]);

        assert_eq!(actions.len(), 2);

        let CodeActionOrCommand::CodeAction(ref action) = actions[0] else {
            panic!("Expected CodeAction");
        };
        assert_eq!(action.title, "Fix field casing: format -> Format");

        let CodeActionOrCommand::CodeAction(ref action) = actions[1] else {
            panic!("Expected CodeAction");
        };
        assert_eq!(
            action.title,
            "Fix field casing: upstream-name -> Upstream-Name"
        );
    }
}
