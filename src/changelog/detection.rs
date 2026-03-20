use tower_lsp_server::ls_types::Uri;

/// Check if a given URL represents a Debian changelog file
pub fn is_changelog_file(uri: &Uri) -> bool {
    let path = uri.as_str();
    path.ends_with("/changelog")
        || path.ends_with("/debian/changelog")
        || path.ends_with("/changelog.dch")
        || path.ends_with("/debian/changelog.dch")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_changelog_file() {
        let changelog_paths = vec![
            "file:///path/to/debian/changelog",
            "file:///project/debian/changelog",
            "file:///changelog",
            "file:///some/path/changelog",
            "file:///path/to/debian/changelog.dch",
            "file:///project/debian/changelog.dch",
            "file:///changelog.dch",
            "file:///some/path/changelog.dch",
        ];

        let non_changelog_paths = vec![
            "file:///path/to/other.txt",
            "file:///path/to/changelog.txt",
            "file:///path/to/mychangelog",
            "file:///path/to/debian/changelog.backup",
        ];

        for path in changelog_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(
                is_changelog_file(&uri),
                "Should detect changelog file: {}",
                path
            );
        }

        for path in non_changelog_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(
                !is_changelog_file(&uri),
                "Should not detect as changelog file: {}",
                path
            );
        }
    }
}
