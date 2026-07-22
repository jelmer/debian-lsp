use tower_lsp_server::ls_types::Uri;

use crate::debhelper::detection::is_debhelper_file;

/// Whether the URI is a debian/not-installed or debian/<package>.not-installed
/// file.
pub fn is_not_installed_file(uri: &Uri) -> bool {
    is_debhelper_file(uri, "not-installed")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    #[test]
    fn detects_qualified_and_unqualified() {
        assert!(is_not_installed_file(&uri(
            "file:///p/debian/not-installed"
        )));
        assert!(is_not_installed_file(&uri(
            "file:///p/debian/mypkg.not-installed"
        )));
    }

    #[test]
    fn rejects_other_files() {
        assert!(!is_not_installed_file(&uri("file:///p/debian/control")));
        assert!(!is_not_installed_file(&uri("file:///p/not-installed")));
        assert!(!is_not_installed_file(&uri(
            "file:///p/debian/not-installed.bak"
        )));
    }
}
