use tower_lsp_server::ls_types::Uri;

/// Check if a given URL represents a Debian copyright file
pub fn is_copyright_file(uri: &Uri) -> bool {
    let path = uri.as_str();
    path.ends_with("/copyright") || path.ends_with("/debian/copyright")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_copyright_file() {
        let copyright_paths = vec![
            "file:///path/to/debian/copyright",
            "file:///project/debian/copyright",
            "file:///copyright",
            "file:///some/path/copyright",
        ];

        let non_copyright_paths = vec![
            "file:///path/to/other.txt",
            "file:///path/to/copyright.txt",
            "file:///path/to/mycopyright",
            "file:///path/to/debian/copyright.backup",
            "file:///path/to/debian/control",
        ];

        for path in copyright_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(
                is_copyright_file(&uri),
                "Should detect copyright file: {}",
                path
            );
        }

        for path in non_copyright_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(
                !is_copyright_file(&uri),
                "Should not detect as copyright file: {}",
                path
            );
        }
    }
}
