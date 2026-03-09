use tower_lsp_server::ls_types::Uri;

/// Check if a given URL represents a debian/upstream/metadata file.
pub fn is_upstream_metadata_file(uri: &Uri) -> bool {
    let path = uri.as_str();
    path.ends_with("/debian/upstream/metadata")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_upstream_metadata_file() {
        let valid_paths = vec![
            "file:///path/to/debian/upstream/metadata",
            "file:///project/debian/upstream/metadata",
        ];

        let invalid_paths = vec![
            "file:///path/to/other.txt",
            "file:///path/to/debian/control",
            "file:///path/to/debian/copyright",
            "file:///path/to/upstream/metadata",
            "file:///path/to/debian/upstream/metadata.bak",
        ];

        for path in valid_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(
                is_upstream_metadata_file(&uri),
                "Should detect upstream/metadata file: {}",
                path
            );
        }

        for path in invalid_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(
                !is_upstream_metadata_file(&uri),
                "Should not detect as upstream/metadata file: {}",
                path
            );
        }
    }
}
