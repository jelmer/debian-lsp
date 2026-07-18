use tower_lsp_server::ls_types::Uri;

use crate::debhelper::detection::is_debhelper_file;

/// Whether the URI is a debian/clean or debian/<package>.clean file.
pub fn is_clean_file(uri: &Uri) -> bool {
    is_debhelper_file(uri, "clean")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    #[test]
    fn detects_qualified_and_unqualified() {
        assert!(is_clean_file(&uri("file:///p/debian/clean")));
        assert!(is_clean_file(&uri("file:///p/debian/mypkg.clean")));
    }

    #[test]
    fn rejects_other_files() {
        assert!(!is_clean_file(&uri("file:///p/debian/control")));
        assert!(!is_clean_file(&uri("file:///p/clean")));
        assert!(!is_clean_file(&uri("file:///p/debian/clean.bak")));
    }
}
