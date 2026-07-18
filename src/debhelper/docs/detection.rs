use tower_lsp_server::ls_types::Uri;

use crate::debhelper::detection::is_debhelper_file;

/// Whether the URI is a debian/docs or debian/<package>.docs file.
pub fn is_docs_file(uri: &Uri) -> bool {
    is_debhelper_file(uri, "docs")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    #[test]
    fn detects_qualified_and_unqualified() {
        assert!(is_docs_file(&uri("file:///p/debian/docs")));
        assert!(is_docs_file(&uri("file:///p/debian/mypkg.docs")));
    }

    #[test]
    fn rejects_other_files() {
        assert!(!is_docs_file(&uri("file:///p/debian/control")));
        assert!(!is_docs_file(&uri("file:///p/docs")));
        assert!(!is_docs_file(&uri("file:///p/debian/docs.bak")));
    }
}
