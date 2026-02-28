use tower_lsp_server::ls_types::Uri;

/// Check if a given URL represents a Debian watch file
pub fn is_watch_file(uri: &Uri) -> bool {
    let path = uri.as_str();
    path.ends_with("/watch") || path.ends_with("/debian/watch")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_watch_file() {
        let watch_paths = vec![
            "file:///path/to/debian/watch",
            "file:///project/debian/watch",
            "file:///watch",
            "file:///some/path/watch",
        ];

        let non_watch_paths = vec![
            "file:///path/to/other.txt",
            "file:///path/to/watch.txt",
            "file:///path/to/mywatch",
            "file:///path/to/debian/watch.backup",
            "file:///path/to/debian/control",
            "file:///path/to/debian/copyright",
        ];

        for path in watch_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(is_watch_file(&uri), "Should detect watch file: {}", path);
        }

        for path in non_watch_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(
                !is_watch_file(&uri),
                "Should not detect as watch file: {}",
                path
            );
        }
    }
}
