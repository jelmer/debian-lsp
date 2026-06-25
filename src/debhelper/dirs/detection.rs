use tower_lsp_server::ls_types::Uri;

/// Check if a given URI represents a debian/dirs or debian/<package>.dirs file.
pub fn is_dirs_file(uri: &Uri) -> bool {
    let path = uri.as_str();
    path.ends_with("/debian/dirs") || path.ends_with(".dirs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_dirs_file() {
        let dirs_paths = vec![
            "file:///path/to/debian/dirs",
            "file:///project/debian/dirs",
            "file:///project/debian/mypackage.dirs",
            "file:///project/debian/my-package.dirs",
        ];
        let non_dirs_paths = vec![
            "file:///path/to/other.txt",
            "file:///path/to/debian/control",
            "file:///path/to/dirs",
            "file:///path/to/debian/dirs.bak",
        ];

        for path in dirs_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(is_dirs_file(&uri), "Should detect dirs file: {}", path);
        }

        for path in non_dirs_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(
                !is_dirs_file(&uri),
                "Should not detect as dirs file: {}",
                path
            );
        }
    }
}
