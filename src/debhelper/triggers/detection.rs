use tower_lsp_server::ls_types::Uri;

use crate::debhelper::detection::is_debhelper_file;

/// Whether the URI is a debian/triggers or debian/<package>.triggers file.
pub fn is_triggers_file(uri: &Uri) -> bool {
    is_debhelper_file(uri, "triggers")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    #[test]
    fn detects_qualified_and_unqualified() {
        assert!(is_triggers_file(&uri("file:///p/debian/triggers")));
        assert!(is_triggers_file(&uri("file:///p/debian/mypkg.triggers")));
    }

    #[test]
    fn rejects_other_files() {
        assert!(!is_triggers_file(&uri("file:///p/debian/control")));
        assert!(!is_triggers_file(&uri("file:///p/triggers")));
        assert!(!is_triggers_file(&uri("file:///p/debian/triggers.bak")));
    }
}
