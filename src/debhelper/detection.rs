//! File detection shared by debhelper config files.

use tower_lsp_server::ls_types::Uri;

/// Whether `uri` names a debhelper config file with the given `stem`.
///
/// debhelper accepts two spellings: the package-qualified
/// `debian/<package>.<stem>` and the unqualified `debian/<stem>` used in the
/// single binary package case. Both are matched here; per-file modules just
/// supply their stem (`"dirs"`, `"install"`, ...).
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

    // Generic stem: the matching is the same for any debhelper file type.
    // The dirs/install stems are checked in their own detection.rs.
    #[test]
    fn test_matches_qualified_and_unqualified() {
        assert!(is_debhelper_file(&uri("file:///p/debian/foo"), "foo"));
        assert!(is_debhelper_file(&uri("file:///p/debian/mypkg.foo"), "foo"));
    }

    #[test]
    fn test_rejects_other_stems_and_paths() {
        assert!(!is_debhelper_file(&uri("file:///p/debian/control"), "foo"));
        assert!(!is_debhelper_file(&uri("file:///p/foo"), "foo"));
        assert!(!is_debhelper_file(&uri("file:///p/debian/foo.bak"), "foo"));
    }
}
