use std::path::{Path, PathBuf};

use tower_lsp_server::ls_types::Uri;

use crate::debhelper::detection::is_debhelper_file;

/// Whether the URI is a debian/links or debian/<package>.links file.
pub fn is_links_file(uri: &Uri) -> bool {
    is_debhelper_file(uri, "links")
}

/// The staging directory whose files a links file refers to: debian/<package>
/// for a debian/<package>.links, else debian/<first-binary-package> for a
/// plain debian/links (dh_link operates on the first binary package listed
/// in debian/control). Returns `None` if the target package can't be
/// determined -- e.g. debian/control is missing or lists no binaries.
pub fn package_dir(debian_dir: &Path, uri: &Uri) -> Option<PathBuf> {
    match package_name(uri) {
        Some(pkg) => Some(debian_dir.join(pkg)),
        None => first_binary_package(debian_dir).map(|pkg| debian_dir.join(pkg)),
    }
}

/// The <package> part of a debian/<package>.links filename, if any.
fn package_name(uri: &Uri) -> Option<String> {
    let file = uri.as_str().rsplit('/').next()?;
    let stem = file.strip_suffix(".links")?;
    (!stem.is_empty()).then(|| stem.to_string())
}

/// The name of the first binary package declared in `debian/control`.
fn first_binary_package(debian_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(debian_dir.join("control")).ok()?;
    debian_control::lossless::Control::parse(&text)
        .tree()
        .binaries()
        .next()?
        .name()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (rel, body) in files {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, body).unwrap();
        }
        dir
    }

    #[test]
    fn detects_qualified_and_unqualified() {
        assert!(is_links_file(&uri("file:///p/debian/links")));
        assert!(is_links_file(&uri("file:///p/debian/mypkg.links")));
    }

    #[test]
    fn qualified_uses_the_package_name_from_the_filename() {
        let debian = Path::new("/p/debian");
        assert_eq!(
            package_dir(debian, &uri("file:///p/debian/mypkg.links")),
            Some(debian.join("mypkg"))
        );
    }

    #[test]
    fn plain_uses_the_first_binary_from_control() {
        let dir = tree(&[(
            "debian/control",
            "Source: src\n\nPackage: firstbin\nArchitecture: any\n\nPackage: secondbin\nArchitecture: any\n",
        )]);
        let debian = dir.path().join("debian");
        assert_eq!(
            package_dir(&debian, &uri("file:///p/debian/links")),
            Some(debian.join("firstbin"))
        );
    }

    #[test]
    fn plain_without_control_returns_none() {
        let dir = tree(&[]);
        let debian = dir.path().join("debian");
        assert_eq!(package_dir(&debian, &uri("file:///p/debian/links")), None);
    }

    #[test]
    fn plain_with_no_binaries_returns_none() {
        let dir = tree(&[("debian/control", "Source: src\n")]);
        let debian = dir.path().join("debian");
        assert_eq!(package_dir(&debian, &uri("file:///p/debian/links")), None);
    }

    #[test]
    fn rejects_other_files() {
        assert!(!is_links_file(&uri("file:///p/debian/control")));
        assert!(!is_links_file(&uri("file:///p/links")));
        assert!(!is_links_file(&uri("file:///p/debian/links.bak")));
    }
}
