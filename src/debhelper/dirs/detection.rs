use tower_lsp_server::ls_types::Uri;

use crate::debhelper::detection::is_debhelper_file;

/// Whether the URI is a debian/dirs or debian/<package>.dirs file.
pub fn is_dirs_file(uri: &Uri) -> bool {
    is_debhelper_file(uri, "dirs")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    #[test]
    fn detects_qualified_and_unqualified() {
        assert!(is_dirs_file(&uri("file:///p/debian/dirs")));
        assert!(is_dirs_file(&uri("file:///p/debian/mypkg.dirs")));
    }

    #[test]
    fn rejects_other_files() {
        assert!(!is_dirs_file(&uri("file:///p/debian/control")));
        assert!(!is_dirs_file(&uri("file:///p/dirs")));
        assert!(!is_dirs_file(&uri("file:///p/debian/dirs.bak")));
    }
}
