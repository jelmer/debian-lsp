use tower_lsp_server::ls_types::Uri;

/// Check if the given URI points to a lintian overrides file.
///
/// Matches:
/// - `debian/source/lintian-overrides`
/// - `debian/<package>.lintian-overrides`
pub fn is_lintian_overrides_file(uri: &Uri) -> bool {
    let path = uri.as_str();
    path.ends_with("/source/lintian-overrides") || path.ends_with(".lintian-overrides")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_lintian_overrides_file() {
        assert!(is_lintian_overrides_file(
            &str::parse("file:///tmp/debian/source/lintian-overrides").unwrap()
        ));
        assert!(is_lintian_overrides_file(
            &str::parse("file:///tmp/debian/mypackage.lintian-overrides").unwrap()
        ));
        assert!(!is_lintian_overrides_file(
            &str::parse("file:///tmp/debian/control").unwrap()
        ));
        assert!(!is_lintian_overrides_file(
            &str::parse("file:///tmp/debian/copyright").unwrap()
        ));
        assert!(!is_lintian_overrides_file(
            &str::parse("file:///tmp/lintian-overrides").unwrap()
        ));
    }

    #[test]
    fn test_source_lintian_overrides() {
        assert!(is_lintian_overrides_file(
            &str::parse("file:///home/user/pkg/debian/source/lintian-overrides").unwrap()
        ));
    }

    #[test]
    fn test_binary_lintian_overrides() {
        assert!(is_lintian_overrides_file(
            &str::parse("file:///home/user/pkg/debian/libfoo1.lintian-overrides").unwrap()
        ));
    }
}
