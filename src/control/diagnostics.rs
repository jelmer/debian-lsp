use text_size::TextRange;
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

use crate::workspace::FieldCasingIssue;

/// Completion context information for a control file field value
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlCompletionContext {
    pub field_name: String,
    pub value_prefix: String,
}

/// All types of diagnostic issues that can be found in a control file
#[derive(Debug, Clone)]
pub enum DiagnosticIssue {
    FieldCasing(FieldCasingIssue),
    ParseError {
        message: String,
        range: Option<TextRange>,
    },
}

/// Get completion context for the field value at the given cursor position in a control file.
///
/// This uses the parsed CST to identify the overlapping entry.
pub fn get_completion_context(
    source_text: &str,
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    position: Position,
) -> Option<ControlCompletionContext> {
    if source_text.is_empty() {
        return None;
    }

    let control = parsed.tree();
    let offset = crate::position::try_position_to_offset(source_text, position)?;
    let text_len = text_size::TextSize::try_from(source_text.len()).ok()?;

    let query_range = if offset >= text_len {
        if text_len == text_size::TextSize::from(0) {
            return None;
        }
        TextRange::new(text_len - text_size::TextSize::from(1), text_len)
    } else {
        TextRange::new(offset, offset + text_size::TextSize::from(1))
    };

    let entry = control.fields_in_range(query_range).next()?;
    let field_name = entry.key()?;
    let colon_range = entry.colon_range()?;

    // Only offer value completions when cursor is at or after the ':' separator.
    if offset < colon_range.end() {
        return None;
    }

    let value_prefix = if let Some(value_range) = entry.value_range() {
        if offset <= value_range.start() {
            String::new()
        } else {
            let prefix_end = if offset < value_range.end() {
                offset
            } else {
                value_range.end()
            };
            let prefix_len: usize = (prefix_end - value_range.start()).into();
            let value = entry.value();
            let mut prefix_bytes = prefix_len.min(value.len());
            while !value.is_char_boundary(prefix_bytes) {
                prefix_bytes -= 1;
            }
            value[..prefix_bytes].to_string()
        }
    } else {
        String::new()
    };

    Some(ControlCompletionContext {
        field_name,
        value_prefix,
    })
}

/// Find all diagnostic issues in a control file, optionally within a specific range
pub fn find_all_issues(
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    range: Option<TextRange>,
) -> Vec<DiagnosticIssue> {
    let mut issues = Vec::new();

    // Add parse errors with position information
    for error in parsed.positioned_errors() {
        // If we have a range filter, check if this error is in range
        if let Some(filter_range) = range {
            if error.range.start() >= filter_range.end()
                || error.range.end() <= filter_range.start()
            {
                continue; // Skip errors outside the range
            }
        }

        issues.push(DiagnosticIssue::ParseError {
            message: error.message.to_string(),
            range: Some(error.range),
        });
    }

    // Add field casing issues
    if let Ok(control) = parsed.clone().to_result() {
        for paragraph in control.as_deb822().paragraphs() {
            for entry in paragraph.entries() {
                let entry_range = entry.text_range();

                // If a range is specified, check if this entry is within it
                if let Some(filter_range) = range {
                    if entry_range.start() >= filter_range.end()
                        || entry_range.end() <= filter_range.start()
                    {
                        continue; // Skip entries outside the range
                    }
                }

                if let Some(field_name) = entry.key() {
                    if let Some(standard_name) =
                        crate::control::get_standard_field_name(&field_name)
                    {
                        if field_name != standard_name {
                            let field_range = TextRange::new(
                                entry_range.start(),
                                entry_range.start() + text_size::TextSize::of(field_name.as_str()),
                            );

                            issues.push(DiagnosticIssue::FieldCasing(FieldCasingIssue {
                                field_name,
                                standard_name: standard_name.to_string(),
                                field_range,
                            }));
                        }
                    }
                }
            }
        }
    }

    issues
}

/// Convert a DiagnosticIssue to an LSP Diagnostic
pub fn issue_to_diagnostic(issue: DiagnosticIssue, source_text: &str) -> Diagnostic {
    match issue {
        DiagnosticIssue::ParseError { message, range } => {
            let lsp_range = if let Some(range) = range {
                crate::position::text_range_to_lsp_range(source_text, range)
            } else {
                // Fallback to (0,0) if no range is available
                Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 0,
                    },
                }
            };

            Diagnostic {
                range: lsp_range,
                severity: Some(DiagnosticSeverity::ERROR),
                message,
                ..Default::default()
            }
        }
        DiagnosticIssue::FieldCasing(casing) => {
            let lsp_range =
                crate::position::text_range_to_lsp_range(source_text, casing.field_range);

            Diagnostic {
                range: lsp_range,
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(NumberOrString::String("field-casing".to_string())),
                source: Some("debian-lsp".to_string()),
                message: format!(
                    "Field name '{}' should be '{}'",
                    casing.field_name, casing.standard_name
                ),
                ..Default::default()
            }
        }
    }
}

/// Find all field casing issues in a control file, optionally within a specific range
pub fn find_field_casing_issues(
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
    range: Option<TextRange>,
) -> Vec<FieldCasingIssue> {
    find_all_issues(parsed, range)
        .into_iter()
        .filter_map(|issue| match issue {
            DiagnosticIssue::FieldCasing(casing) => Some(casing),
            _ => None,
        })
        .collect()
}

/// Get all LSP diagnostics for a control file
pub fn get_diagnostics(
    source_text: &str,
    parsed: &debian_control::lossless::Parse<debian_control::lossless::Control>,
) -> Vec<Diagnostic> {
    find_all_issues(parsed, None)
        .into_iter()
        .map(|issue| issue_to_diagnostic(issue, source_text))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;

    #[test]
    fn test_find_all_issues_correct_casing() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/control").unwrap();
        let content = "Source: test-package\nMaintainer: Test <test@example.com>\n\nPackage: test-package\nArchitecture: amd64\nDescription: A test package\n";
        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_control(file);

        let issues = find_all_issues(&parsed, None);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_find_all_issues_incorrect_casing() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/control").unwrap();
        let content = "source: test-package\nmaintainer: Test <test@example.com>\n\npackage: test-package\narchitecture: amd64\ndescription: A test package\n";
        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_control(file);

        let issues = find_all_issues(&parsed, None);
        assert!(!issues.is_empty());
    }

    #[test]
    fn test_completion_context_for_section_value() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/control").unwrap();
        let content = "Source: test\nSection: py\n";
        let file = workspace.update_file(url, content.to_string());
        let source_text = workspace.source_text(file);
        let parsed = workspace.get_parsed_control(file);

        let context = get_completion_context(&source_text, &parsed, Position::new(1, 11))
            .expect("Should have completion context");

        assert_eq!(context.field_name, "Section");
        assert_eq!(context.value_prefix, "py");
    }

    #[test]
    fn test_completion_context_immediately_after_colon() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/control").unwrap();
        let content = "Section: py\n";
        let file = workspace.update_file(url, content.to_string());
        let source_text = workspace.source_text(file);
        let parsed = workspace.get_parsed_control(file);

        let context = get_completion_context(&source_text, &parsed, Position::new(0, 8))
            .expect("Should have completion context");

        assert_eq!(context.field_name, "Section");
        assert_eq!(context.value_prefix, "");
    }

    #[test]
    fn test_completion_context_for_priority_value() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/control").unwrap();
        let content = "Source: test\nPriority: op\n";
        let file = workspace.update_file(url, content.to_string());
        let source_text = workspace.source_text(file);
        let parsed = workspace.get_parsed_control(file);

        let context = get_completion_context(&source_text, &parsed, Position::new(1, 12))
            .expect("Should have completion context");

        assert_eq!(context.field_name, "Priority");
        assert_eq!(context.value_prefix, "op");
    }

    #[test]
    fn test_completion_context_none_in_field_key() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/control").unwrap();
        let content = "Source: test\nSection: py\n";
        let file = workspace.update_file(url, content.to_string());
        let source_text = workspace.source_text(file);
        let parsed = workspace.get_parsed_control(file);

        let context = get_completion_context(&source_text, &parsed, Position::new(1, 3));
        assert!(context.is_none());
    }
}
