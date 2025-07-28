use tower_lsp::lsp_types::Url;

/// Check if a given URL represents a Debian control file
pub fn is_control_file(uri: &Url) -> bool {
    uri.path().ends_with("/control") || uri.path().ends_with("/debian/control")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_control_file() {
        let control_paths = vec![
            "file:///path/to/debian/control",
            "file:///project/debian/control",
            "file:///control",
            "file:///some/path/control",
        ];

        let non_control_paths = vec![
            "file:///path/to/other.txt",
            "file:///path/to/control.txt",
            "file:///path/to/mycontrol",
            "file:///path/to/debian/control.backup",
        ];

        for path in control_paths {
            let uri = Url::parse(path).unwrap();
            assert!(
                is_control_file(&uri),
                "Should detect control file: {}",
                path
            );
        }

        for path in non_control_paths {
            let uri = Url::parse(path).unwrap();
            assert!(
                !is_control_file(&uri),
                "Should not detect as control file: {}",
                path
            );
        }
    }
}
