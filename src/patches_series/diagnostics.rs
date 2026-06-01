use super::detection::list_patch_files;
use crate::position::Source;
use text_size::TextRange;
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Uri};

/// All types of diagnostic issues that can be found in a patches/series file
#[derive(Debug, Clone)]
pub enum DiagnosticIssue {
    ParseError {
        message: String,
        range: TextRange,
    },
    ParseWarning {
        message: String,
        range: TextRange,
    },
    MissingPatch {
        patch_name: String,
        range: TextRange,
    },
}

/// Find all diagnostic issues in a patches/series file, optionally within a specific range
pub fn find_all_issues(
    uri: &Uri,
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
            range: error.position,
        });
    }

    for warning in parsed.positioned_warnings() {
        if let Some(filter_range) = range {
            if warning.position.start() >= filter_range.end()
                || warning.position.end() <= filter_range.start()
            {
                continue;
            }
        }

        issues.push(DiagnosticIssue::ParseWarning {
            message: warning.message.to_string(),
            range: warning.position,
        });
    }

    let patch_files = list_patch_files(uri);
    let series = parsed.tree();
    for patch in series.patch_entries() {
        if let Some(name) = patch.name() {
            if !patch_files.contains(&name) {
                if let Some(token) = patch.name_token() {
                    let range = token.text_range();
                    issues.push(DiagnosticIssue::MissingPatch {
                        patch_name: name,
                        range: range,
                    });
                }
            }
        }
    }

    issues
}

/// Convert a DiagnosticIssue to an LSP Diagnostic
pub fn issue_to_diagnostic(issue: DiagnosticIssue, src: Source<'_>) -> Diagnostic {
    match issue {
        DiagnosticIssue::ParseError { message, range } => {
            let lsp_range = src.text_range_to_lsp_range(range);

            Diagnostic {
                range: lsp_range,
                severity: Some(DiagnosticSeverity::ERROR),
                code: Some(NumberOrString::String("parse-error".to_string())),
                source: Some("debian-lsp".to_string()),
                message,
                ..Default::default()
            }
        }
        DiagnosticIssue::ParseWarning { message, range } => {
            let lsp_range = src.text_range_to_lsp_range(range);

            Diagnostic {
                range: lsp_range,
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(NumberOrString::String("parse-warning".to_string())),
                source: Some("debian-lsp".to_string()),
                message,
                ..Default::default()
            }
        }
        DiagnosticIssue::MissingPatch { patch_name, range } => {
            let lsp_range = src.text_range_to_lsp_range(range);

            Diagnostic {
                range: lsp_range,
                severity: Some(DiagnosticSeverity::ERROR),
                code: Some(NumberOrString::String("missing-patch".to_string())),
                source: Some("debian-lsp".to_string()),
                message: format!("Patch '{}' not found", patch_name),
                ..Default::default()
            }
        }
    }
}

/// Get all LSP diagnostics for a control file
pub fn get_diagnostics(
    uri: &Uri,
    src: Source<'_>,
    parsed: &patchkit::edit::Parse<patchkit::edit::series::lossless::SeriesFile>,
) -> Vec<Diagnostic> {
    find_all_issues(uri, parsed, None)
        .into_iter()
        .map(|issue| issue_to_diagnostic(issue, src))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_uri() -> Uri {
        "file:///debian/patches/series".parse().unwrap()
    }

    #[test]
    fn test_warning_duplicate_patch() {
        let parsed = patchkit::edit::series::parse("patch1.patch\npatch1.patch\n");
        let issues = find_all_issues(&make_uri(), &parsed, None);
        assert!(!issues.is_empty());
    }

    #[test]
    fn test_error_unexpected_patch_name() {
        let parsed = patchkit::edit::series::parse("fix.patch other.patch\n");
        let issues = find_all_issues(&make_uri(), &parsed, None);
        assert!(!issues.is_empty());
    }

    #[test]
    fn test_warning_invalid_option() {
        let parsed = patchkit::edit::series::parse("fix.patch -aa\n");
        let issues = find_all_issues(&make_uri(), &parsed, None);
        assert!(!issues.is_empty());
    }

    #[test]
    fn test_warning_invalid_strip_option() {
        let parsed = patchkit::edit::series::parse("fix.patch -p4\n");
        let issues = find_all_issues(&make_uri(), &parsed, None);
        assert!(!issues.is_empty());
    }

    #[test]
    fn test_missing_patch() {
        let parsed = patchkit::edit::series::parse("nonexistent.patch\n");
        let issues = find_all_issues(&make_uri(), &parsed, None);
        assert!(!issues.is_empty());
    }
}
