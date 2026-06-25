use crate::debhelper::diagnostics;
use crate::position::Source;
use tower_lsp_server::ls_types::Diagnostic;

/// Dedup key for a debian/dirs entry: ignore a leading slash so `/usr/bin`
/// and `usr/bin` count as the same directory.
fn dirs_key(trimmed: &str) -> String {
    trimmed.trim_start_matches('/').to_string()
}

/// Get all LSP diagnostics for a debian/dirs file.
pub fn get_diagnostics(src: Source<'_>) -> Vec<Diagnostic> {
    diagnostics::get_diagnostics(src, dirs_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debhelper::diagnostics::{find_duplicate_entries, DiagnosticIssue};
    use crate::position::LineIndex;

    fn issues(text: &str) -> Vec<DiagnosticIssue> {
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        find_duplicate_entries(src, dirs_key)
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
