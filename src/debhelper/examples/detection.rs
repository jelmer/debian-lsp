use tower_lsp_server::ls_types::Uri;

use crate::debhelper::detection::is_debhelper_file;

/// Whether the URI is a debian/examples or debian/<package>.examples file.
pub fn is_examples_file(uri: &Uri) -> bool {
    is_debhelper_file(uri, "examples")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    #[test]
    fn detects_qualified_and_unqualified() {
        assert!(is_examples_file(&uri("file:///p/debian/examples")));
        assert!(is_examples_file(&uri("file:///p/debian/mypkg.examples")));
    }

    #[test]
    fn rejects_other_files() {
        assert!(!is_examples_file(&uri("file:///p/debian/control")));
        assert!(!is_examples_file(&uri("file:///p/examples")));
        assert!(!is_examples_file(&uri("file:///p/debian/examples.bak")));
    }
}
