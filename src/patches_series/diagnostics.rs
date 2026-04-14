use text_size::TextRange;
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, Position, Range};

/// All types of diagnostic issues that can be found in a patches/series file
#[derive(Debug, Clone)]
pub enum DiagnosticIssue {
    ParseError {
        message: String,
        range: Option<TextRange>,
    },
}

/// Find all diagnostic issues in a patches/series file, optionally within a specific range
pub fn find_all_issues(
    parsed: &patchkit::edit::Parse<patchkit::edit::series::lossless::SeriesFile>,
    range: Option<TextRange>,
) -> Vec<DiagnosticIssue> {
    let mut issues = Vec::new();

    // Add parse errors with position information
    for error in parsed.positioned_errors() {
        if let Some(filter_range) = range {
            if error.position.start() >= filter_range.end()
                || error.position.end() <= filter_range.start()
            {
                continue;
            }
        }

        issues.push(DiagnosticIssue::ParseError {
            message: error.message.to_string(),
            range: Some(error.position),
        });
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
    }
}

/// Get all LSP diagnostics for a patches/series file
pub fn get_diagnostics(
    source_text: &str,
    parsed: &patchkit::edit::Parse<patchkit::edit::series::lossless::SeriesFile>,
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
    fn test_find_all_issues_valid_series() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/patches/series").unwrap();
        let content = "fix-build.patch\nadd-feature.patch\n";
        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_patches_series(file);

        let issues = find_all_issues(&parsed, None);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_find_all_issues_empty_series() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/patches/series").unwrap();
        let content = "";
        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_patches_series(file);

        let issues = find_all_issues(&parsed, None);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_find_all_issues_with_comments() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/patches/series").unwrap();
        let content =
            "# This is a comment\nfix-build.patch\n# Another comment\nadd-feature.patch\n";
        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_patches_series(file);

        let issues = find_all_issues(&parsed, None);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_find_all_issues_range_filter() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/patches/series").unwrap();
        let content = "fix-build.patch\nadd-feature.patch\n";
        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_patches_series(file);

        let range = TextRange::new(0.into(), 15.into());
        let issues = find_all_issues(&parsed, Some(range));
        assert!(issues.is_empty());
    }

    #[test]
    fn test_get_diagnostics_valid_series() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/patches/series").unwrap();
        let content = "fix-build.patch\nadd-feature.patch\n";
        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_patches_series(file);

        let diagnostics = get_diagnostics(content, &parsed);
        assert!(diagnostics.is_empty());
    }
}
