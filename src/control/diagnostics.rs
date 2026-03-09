use text_size::TextRange;
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

use crate::workspace::FieldCasingIssue;

/// All types of diagnostic issues that can be found in a control file
#[derive(Debug, Clone)]
pub enum DiagnosticIssue {
    FieldCasing(FieldCasingIssue),
    ParseError {
        message: String,
        range: Option<TextRange>,
    },
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
}
