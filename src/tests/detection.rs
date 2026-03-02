use tower_lsp_server::ls_types::Uri;

/// Check if a given URL represents a Debian tests/control file
pub fn is_tests_control_file(uri: &Uri) -> bool {
    let path = uri.as_str();
    path.ends_with("/debian/tests/control")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_tests_control_file() {
        let tests_control_paths = vec![
            "file:///path/to/debian/tests/control",
            "file:///project/debian/tests/control",
        ];

        let non_tests_control_paths = vec![
            "file:///path/to/other.txt",
            "file:///path/to/debian/control",
            "file:///path/to/debian/copyright",
            "file:///path/to/debian/watch",
            "file:///path/to/tests/control", // Not in debian/ directory
            "file:///path/to/debian/tests/control.backup",
        ];

        for path in tests_control_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(
                is_tests_control_file(&uri),
                "Should detect tests/control file: {}",
                path
            );
        }

        for path in non_tests_control_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(
                !is_tests_control_file(&uri),
                "Should not detect as tests/control file: {}",
                path
            );
        }
    }
}
