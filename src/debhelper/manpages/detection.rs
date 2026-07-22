use tower_lsp_server::ls_types::Uri;

use crate::debhelper::detection::is_debhelper_file;

/// Whether the URI is a debian/manpages or debian/<package>.manpages file.
pub fn is_manpages_file(uri: &Uri) -> bool {
    is_debhelper_file(uri, "manpages")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    #[test]
    fn detects_qualified_and_unqualified() {
        assert!(is_manpages_file(&uri("file:///p/debian/manpages")));
        assert!(is_manpages_file(&uri("file:///p/debian/mypkg.manpages")));
    }

    #[test]
    fn rejects_other_files() {
        assert!(!is_manpages_file(&uri("file:///p/debian/control")));
        assert!(!is_manpages_file(&uri("file:///p/manpages")));
        assert!(!is_manpages_file(&uri("file:///p/debian/manpages.bak")));
    }
}
