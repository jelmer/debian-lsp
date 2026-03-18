use tower_lsp_server::ls_types::Uri;

/// Check if a given URL represents a debian/rules file.
pub fn is_rules_file(uri: &Uri) -> bool {
    let path = uri.as_str();
    path.ends_with("/debian/rules")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_rules_file() {
        let valid_paths = vec![
            "file:///path/to/debian/rules",
            "file:///project/debian/rules",
        ];

        let invalid_paths = vec![
            "file:///path/to/other.txt",
            "file:///path/to/debian/control",
            "file:///path/to/debian/copyright",
            "file:///path/to/rules",
            "file:///path/to/debian/rules.bak",
        ];

        for path in valid_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(is_rules_file(&uri), "Should detect rules file: {}", path);
        }

        for path in invalid_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(
                !is_rules_file(&uri),
                "Should not detect as rules file: {}",
                path
            );
        }
    }
}
