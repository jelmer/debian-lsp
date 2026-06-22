//! Diagnostics shared by line-based debhelper config files.
//!
//! Every debhelper helper that reads a one-entry-per-line file (dirs,
//! install, and the ones still to come) wants the same duplicate-entry
//! check. What counts as "the same entry" is the only thing that varies,
//! so each module passes in its own normalization closure.

use crate::position::Source;
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

/// A problem found in a line-based debhelper config file.
#[derive(Debug, Clone)]
pub enum DiagnosticIssue {
    /// The same entry is listed more than once.
    DuplicateEntry { entry: String, range: Range },
}

/// Scan the meaningful lines of a debhelper file and flag any whose
/// normalized form has already been seen.
///
/// Blank lines and `#` comments are ignored, as debhelper(7) requires.
/// `normalize` maps a trimmed line to the key that decides what counts as a
/// duplicate: dirs ignores a leading slash, install collapses internal
/// whitespace, and so on.
pub fn find_duplicate_entries<F>(src: Source<'_>, normalize: F) -> Vec<DiagnosticIssue>
where
    F: Fn(&str) -> String,
{
    let mut issues = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for (line_num, line) in src.text.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if !seen.insert(normalize(trimmed)) {
            issues.push(DiagnosticIssue::DuplicateEntry {
                entry: trimmed.to_string(),
                range: line_range(src, line_num),
            });
        }
    }

    issues
}

/// Build the LSP range covering an entire line.
fn line_range(src: Source<'_>, line_num: usize) -> Range {
    let line = src.text.lines().nth(line_num).unwrap_or("");
    let start = Position::new(line_num as u32, 0);
    let end = Position::new(line_num as u32, crate::position::utf16_len(line));
    Range::new(start, end)
}

/// Turn an issue into the LSP diagnostic shown in the editor.
pub fn issue_to_diagnostic(issue: DiagnosticIssue) -> Diagnostic {
    match issue {
        DiagnosticIssue::DuplicateEntry { entry, range } => Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String("duplicate-entry".to_string())),
            source: Some("debian-lsp".to_string()),
            message: format!("Duplicate entry '{}'", entry),
            ..Default::default()
        },
    }
}

/// Collect the LSP diagnostics for a debhelper file, using `normalize` to
/// decide what counts as a duplicate entry.
pub fn get_diagnostics<F>(src: Source<'_>, normalize: F) -> Vec<Diagnostic>
where
    F: Fn(&str) -> String,
{
    find_duplicate_entries(src, normalize)
        .into_iter()
        .map(issue_to_diagnostic)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;

    fn issues(text: &str, normalize: impl Fn(&str) -> String) -> Vec<DiagnosticIssue> {
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        find_duplicate_entries(src, normalize)
    }

    #[test]
    fn test_flags_exact_duplicate() {
        let found = issues("usr/bin\nusr/bin\n", str::to_string);
        assert_eq!(found.len(), 1);
        assert!(matches!(found[0], DiagnosticIssue::DuplicateEntry { .. }));
    }

    #[test]
    fn test_normalize_controls_what_is_a_duplicate() {
        // With a slash-stripping key, "/usr/bin" and "usr/bin" collide.
        let found = issues("usr/bin\n/usr/bin\n", |l| {
            l.trim_start_matches('/').to_string()
        });
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn test_blank_and_comment_lines_ignored() {
        let found = issues("\n# a comment\nusr/bin\n", str::to_string);
        assert!(found.is_empty());
    }

    #[test]
    fn test_issue_to_diagnostic_is_a_warning() {
        let found = issues("usr/bin\nusr/bin\n", str::to_string);
        let diag = issue_to_diagnostic(found.into_iter().next().unwrap());
        assert_eq!(diag.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            diag.code,
            Some(NumberOrString::String("duplicate-entry".to_string()))
        );
        assert!(diag.message.contains("usr/bin"));
    }
}
