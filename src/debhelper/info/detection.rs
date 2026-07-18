use tower_lsp_server::ls_types::Uri;

use crate::debhelper::detection::is_debhelper_file;

/// Whether the URI is a debian/info or debian/<package>.info file.
pub fn is_info_file(uri: &Uri) -> bool {
    is_debhelper_file(uri, "info")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    #[test]
    fn detects_qualified_and_unqualified() {
        assert!(is_info_file(&uri("file:///p/debian/info")));
        assert!(is_info_file(&uri("file:///p/debian/mypkg.info")));
    }

    #[test]
    fn rejects_other_files() {
        assert!(!is_info_file(&uri("file:///p/debian/control")));
        assert!(!is_info_file(&uri("file:///p/info")));
        assert!(!is_info_file(&uri("file:///p/debian/info.bak")));
    }
}
