use tower_lsp_server::ls_types::Uri;

/// Check if the given URI points to a debian/source/format file
pub fn is_source_format_file(uri: &Uri) -> bool {
    let path = uri.as_str();
    path.ends_with("/source/format") || path.ends_with("/debian/source/format")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_source_format_file() {
        assert!(is_source_format_file(
            &str::parse("file:///tmp/debian/source/format").unwrap()
        ));
        assert!(is_source_format_file(
            &str::parse("file:///tmp/source/format").unwrap()
        ));
        assert!(!is_source_format_file(
            &str::parse("file:///tmp/debian/control").unwrap()
        ));
        assert!(!is_source_format_file(
            &str::parse("file:///tmp/format").unwrap()
        ));
    }
}
