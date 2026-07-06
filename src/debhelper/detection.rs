use tower_lsp_server::ls_types::Uri;

/// Check if a given URI represents a debhelper file with the given stem, i.e.
/// `debian/<stem>` or `debian/<package>.<stem>`.
pub fn is_debhelper_file(uri: &Uri, stem: &str) -> bool {
    let path = uri.as_str();
    path.ends_with(&format!("/debian/{stem}")) || path.ends_with(&format!(".{stem}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    // Generic stem: the matching is the same for any debhelper file type. Each
    // file type checks its own stem in its own detection.rs.
    #[test]
    fn matches_qualified_and_unqualified() {
        assert!(is_debhelper_file(&uri("file:///p/debian/foo"), "foo"));
        assert!(is_debhelper_file(&uri("file:///p/debian/mypkg.foo"), "foo"));
    }

    #[test]
    fn rejects_other_stems_and_paths() {
        assert!(!is_debhelper_file(&uri("file:///p/debian/control"), "foo"));
        assert!(!is_debhelper_file(&uri("file:///p/foo"), "foo"));
        assert!(!is_debhelper_file(&uri("file:///p/debian/foo.bak"), "foo"));
    }
}
