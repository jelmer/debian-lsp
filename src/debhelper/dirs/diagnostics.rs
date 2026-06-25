use crate::position::Source;
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Range};

/// All types of diagnostic issues in a debian/dirs file.
#[derive(Debug, Clone)]
pub enum DiagnosticIssue {
    /// Duplicate directory entry
    DuplicateEntry { path: String, range: Range },
}

/// Find all diagnostic issues in a debian/dirs file.
pub fn find_all_issues(src: Source<'_>) -> Vec<DiagnosticIssue> {
    let mut issues = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for (line_num, line) in src.text.lines().enumerate() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Normalize: strip leading slash for dedup key
        let key = trimmed.trim_start_matches('/').to_string();
        if !seen.insert(key) {
            issues.push(DiagnosticIssue::DuplicateEntry {
                path: trimmed.to_string(),
                range: line_range(src, line_num),
            });
        }
    }

    issues
}

/// Build the LSP Range for an entire line.
fn line_range(src: Source<'_>, line_num: usize) -> Range {
    let line = src.text.lines().nth(line_num).unwrap_or("");
    let start = tower_lsp_server::ls_types::Position::new(line_num as u32, 0);
    let end = tower_lsp_server::ls_types::Position::new(
        line_num as u32,
        crate::position::utf16_len(line),
    );
    Range::new(start, end)
}

/// Convert a DiagnosticIssue to an LSP Diagnostic.
pub fn issue_to_diagnostic(issue: DiagnosticIssue) -> Diagnostic {
    match issue {
        DiagnosticIssue::DuplicateEntry { path, range } => Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String("duplicate-entry".to_string())),
            source: Some("debian-lsp".to_string()),
            message: format!("Duplicate entry '{}'", path),
            ..Default::default()
        },
    }
}

/// Get all LSP diagnostics for a debian/dirs file.
pub fn get_diagnostics(src: Source<'_>) -> Vec<Diagnostic> {
    find_all_issues(src)
        .into_iter()
        .map(issue_to_diagnostic)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;

    fn issues(text: &str) -> Vec<DiagnosticIssue> {
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        find_all_issues(src)
    }

    #[test]
    fn test_duplicate_entry_is_warning() {
        let diags = issues("usr/share/myapp\nusr/share/myapp\n");
        assert!(diags
            .iter()
            .any(|d| matches!(d, DiagnosticIssue::DuplicateEntry { .. })));
    }

    #[test]
    fn test_duplicate_with_leading_slash_is_warning() {
        let diags = issues("usr/share/myapp\n/usr/share/myapp\n");
        assert!(diags
            .iter()
            .any(|d| matches!(d, DiagnosticIssue::DuplicateEntry { .. })));
    }

    #[test]
    fn test_no_duplicate() {
        let diags = issues("usr/share/myapp\nusr/lib/myapp\n");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_empty_line_ignored() {
        let diags = issues("\nusr/share/myapp\n");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_comment_ignored() {
        let diags = issues("# this is a comment\nusr/share/myapp\n");
        assert!(diags.is_empty());
    }
}
