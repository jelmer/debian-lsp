use tower_lsp_server::ls_types::Uri;

/// Check if a given URL represents a Debian changelog file
pub fn is_changelog_file(uri: &Uri) -> bool {
    let path = uri.as_str();
    path.ends_with("/changelog")
        || path.ends_with("/debian/changelog")
        || path.ends_with("/changelog.dch")
        || path.ends_with("/debian/changelog.dch")
}

/// Check if a given URL represents a Debian NEWS file
/// (`debian/NEWS` or `debian/<package>.NEWS`).
///
/// NEWS files share the syntax of `debian/changelog` and are parsed with the
/// same parser, but are kept as their own file type so dispatch can
/// distinguish them.
pub fn is_news_file(uri: &Uri) -> bool {
    let path = uri.as_str();
    let Some(name) = path.rsplit('/').next() else {
        return false;
    };
    if name != "NEWS" && !name.ends_with(".NEWS") {
        return false;
    }
    path.rsplitn(3, '/')
        .nth(1)
        .is_some_and(|parent| parent == "debian")
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
            "file:///path/to/debian/NEWS",
            "file:///path/to/debian/foo.NEWS",
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

    #[test]
    fn test_is_news_file() {
        let news_paths = vec![
            "file:///path/to/debian/NEWS",
            "file:///project/debian/NEWS",
            "file:///path/to/debian/foo.NEWS",
            "file:///project/debian/libfoo-dev.NEWS",
        ];

        let non_news_paths = vec![
            "file:///path/to/debian/changelog",
            "file:///path/to/NEWS",
            "file:///project/NEWS",
            "file:///path/to/foo.NEWS",
            "file:///path/to/debian/news",
            "file:///path/to/debian/NEWS.old",
        ];

        for path in news_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(is_news_file(&uri), "Should detect NEWS file: {}", path);
        }

        for path in non_news_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(
                !is_news_file(&uri),
                "Should not detect as NEWS file: {}",
                path
            );
        }
    }
}
