use std::path::{Path, PathBuf};

use tower_lsp_server::ls_types::Uri;

use crate::debhelper::detection::is_debhelper_file;

/// Whether the URI is a debian/links or debian/<package>.links file.
pub fn is_links_file(uri: &Uri) -> bool {
    is_debhelper_file(uri, "links")
}

/// The staging directory whose files a links file refers to: debian/<package>
/// for a debian/<package>.links file, else debian/tmp for a plain debian/links.
pub fn package_dir(debian_dir: &Path, uri: &Uri) -> PathBuf {
    match package_name(uri) {
        Some(pkg) => debian_dir.join(pkg),
        None => debian_dir.join("tmp"),
    }
}

/// The <package> part of a debian/<package>.links filename, if any.
fn package_name(uri: &Uri) -> Option<String> {
    let file = uri.as_str().rsplit('/').next()?;
    let stem = file.strip_suffix(".links")?;
    (!stem.is_empty()).then(|| stem.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    #[test]
    fn detects_qualified_and_unqualified() {
        assert!(is_links_file(&uri("file:///p/debian/links")));
        assert!(is_links_file(&uri("file:///p/debian/mypkg.links")));
    }

    #[test]
    fn package_dir_uses_the_package_name() {
        let debian = Path::new("/p/debian");
        assert_eq!(
            package_dir(debian, &uri("file:///p/debian/mypkg.links")),
            debian.join("mypkg")
        );
        assert_eq!(
            package_dir(debian, &uri("file:///p/debian/links")),
            debian.join("tmp")
        );
    }

    #[test]
    fn rejects_other_files() {
        assert!(!is_links_file(&uri("file:///p/debian/control")));
        assert!(!is_links_file(&uri("file:///p/links")));
        assert!(!is_links_file(&uri("file:///p/debian/links.bak")));
    }
}
