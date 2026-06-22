use crate::debhelper::diagnostics;
use crate::position::Source;
use tower_lsp_server::ls_types::Diagnostic;

/// Dedup key for a debian/install entry: collapse internal whitespace so
/// `foo  usr/bin` and `foo usr/bin` count as the same entry.
///
/// The check is deliberately limited to duplicates. Anything needing the
/// source or build tree (does the source path exist? is the destination
/// reachable?) can't be assessed reliably at edit time, and a leading slash
/// on the destination is perfectly valid.
fn install_key(trimmed: &str) -> String {
    trimmed.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Get all LSP diagnostics for a debian/install file.
pub fn get_diagnostics(src: Source<'_>) -> Vec<Diagnostic> {
    diagnostics::get_diagnostics(src, install_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debhelper::diagnostics::{find_duplicate_entries, DiagnosticIssue};
    use crate::position::LineIndex;

    fn issues(text: &str) -> Vec<DiagnosticIssue> {
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        find_duplicate_entries(src, install_key)
    }

    #[test]
    fn test_duplicate_entry_is_warning() {
        let diags = issues("my-prog usr/bin\nmy-prog usr/bin\n");
        assert_eq!(diags.len(), 1);
        assert!(matches!(diags[0], DiagnosticIssue::DuplicateEntry { .. }));
    }

    #[test]
    fn test_duplicate_with_whitespace_variation() {
        let diags = issues("my-prog usr/bin\nmy-prog   usr/bin\n");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_same_source_different_dest_is_not_duplicate() {
        let diags = issues("my-prog usr/bin\nmy-prog usr/sbin\n");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_no_duplicate() {
        let diags = issues("a usr/bin\nb usr/lib\n");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_blank_lines_ignored() {
        let diags = issues("\nmy-prog usr/bin\n\n");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_comments_ignored() {
        // debhelper(7): lines starting with '#' are comments.
        let diags = issues("# install the main binary\nmy-prog usr/bin\n");
        assert!(diags.is_empty());
    }

    #[test]
    fn test_single_token_autodest_entry() {
        // A lone source path (autodest) is a valid entry, not malformed.
        let diags = issues("debian/tmp/usr/bin/my-prog\n");
        assert!(diags.is_empty());
    }
}
