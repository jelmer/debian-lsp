use tower_lsp_server::ls_types::Uri;

/// Check if the given URI points to a conffiles file.
///
/// Matches:
/// - `debian/conffiles`
/// - `debian/<package>.conffiles`
pub fn is_conffiles_file(uri: &Uri) -> bool {
    let path = uri.as_str();
    path.ends_with("/debian/conffiles") || path.ends_with(".conffiles")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_conffiles_file() {
        assert!(is_conffiles_file(
            &str::parse("file:///tmp/debian/conffiles").unwrap()
        ));
        assert!(is_conffiles_file(
            &str::parse("file:///tmp/debian/mypackage.conffiles").unwrap()
        ));
        assert!(!is_conffiles_file(
            &str::parse("file:///tmp/debian/control").unwrap()
        ));
        assert!(!is_conffiles_file(
            &str::parse("file:///tmp/debian/copyright").unwrap()
        ));
        assert!(!is_conffiles_file(
            &str::parse("file:///tmp/conffiles").unwrap()
        ));
    }

    #[test]
    fn test_simple_conffiles() {
        assert!(is_conffiles_file(
            &str::parse("file:///home/user/pkg/debian/conffiles").unwrap()
        ));
    }

    #[test]
    fn test_binary_conffiles() {
        assert!(is_conffiles_file(
            &str::parse("file:///home/user/pkg/debian/libfoo1.conffiles").unwrap()
        ));
    }
}
