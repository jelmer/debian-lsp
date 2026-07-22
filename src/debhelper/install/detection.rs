use tower_lsp_server::ls_types::Uri;

use crate::debhelper::detection::is_debhelper_file;

/// Whether the URI is a debian/install or debian/<package>.install file.
pub fn is_install_file(uri: &Uri) -> bool {
    is_debhelper_file(uri, "install")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    #[test]
    fn detects_qualified_and_unqualified() {
        assert!(is_install_file(&uri("file:///p/debian/install")));
        assert!(is_install_file(&uri("file:///p/debian/mypkg.install")));
    }

    #[test]
    fn rejects_other_files() {
        assert!(!is_install_file(&uri("file:///p/debian/control")));
        assert!(!is_install_file(&uri("file:///p/install")));
        assert!(!is_install_file(&uri("file:///p/debian/install.bak")));
    }
}
