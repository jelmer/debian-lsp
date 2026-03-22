use tower_lsp_server::ls_types::Uri;

/// Check if the given URI points to a debian/source/options file
pub fn is_source_options_file(uri: &Uri) -> bool {
    let path = uri.as_str();
    path.ends_with("/source/options") || path.ends_with("/debian/source/options")
}

/// Check if the given URI points to a debian/source/local-options file
pub fn is_source_local_options_file(uri: &Uri) -> bool {
    let path = uri.as_str();
    path.ends_with("/source/local-options") || path.ends_with("/debian/source/local-options")
}

/// Check if the given URI points to a debian/source/options or local-options file
pub fn is_source_options_or_local_options_file(uri: &Uri) -> bool {
    is_source_options_file(uri) || is_source_local_options_file(uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_source_options_file() {
        assert!(is_source_options_file(
            &str::parse("file:///tmp/debian/source/options").unwrap()
        ));
        assert!(is_source_options_file(
            &str::parse("file:///tmp/source/options").unwrap()
        ));
        assert!(!is_source_options_file(
            &str::parse("file:///tmp/debian/control").unwrap()
        ));
        assert!(!is_source_options_file(
            &str::parse("file:///tmp/options").unwrap()
        ));
        assert!(!is_source_options_file(
            &str::parse("file:///tmp/debian/source/local-options").unwrap()
        ));
    }

    #[test]
    fn test_is_source_local_options_file() {
        assert!(is_source_local_options_file(
            &str::parse("file:///tmp/debian/source/local-options").unwrap()
        ));
        assert!(is_source_local_options_file(
            &str::parse("file:///tmp/source/local-options").unwrap()
        ));
        assert!(!is_source_local_options_file(
            &str::parse("file:///tmp/debian/source/options").unwrap()
        ));
    }

    #[test]
    fn test_is_source_options_or_local_options_file() {
        assert!(is_source_options_or_local_options_file(
            &str::parse("file:///tmp/debian/source/options").unwrap()
        ));
        assert!(is_source_options_or_local_options_file(
            &str::parse("file:///tmp/debian/source/local-options").unwrap()
        ));
        assert!(!is_source_options_or_local_options_file(
            &str::parse("file:///tmp/debian/control").unwrap()
        ));
    }
}
