use crate::debhelper::detection::is_debhelper_file;
use tower_lsp_server::ls_types::Uri;

/// Check if a given URI represents a debian/install or
/// debian/<package>.install file.
///
/// `debian/install` (without a package name) is valid in the single
/// binary package case via debhelper(7)'s generic fallback: when there is
/// only one binary package, debhelper uses `debian/foo` if there is no
/// `debian/package.foo`. dh_install(1) itself documents the
/// package-qualified form.
pub fn is_install_file(uri: &Uri) -> bool {
    is_debhelper_file(uri, "install")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_install_file() {
        let install_paths = vec![
            "file:///path/to/debian/install",
            "file:///project/debian/install",
            "file:///project/debian/mypackage.install",
            "file:///project/debian/my-package.install",
        ];
        let non_install_paths = vec![
            "file:///path/to/other.txt",
            "file:///path/to/debian/control",
            "file:///path/to/install",
            "file:///path/to/debian/install.bak",
        ];

        for path in install_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(is_install_file(&uri), "Should detect install file: {}", path);
        }
        for path in non_install_paths {
            let uri = path.parse::<Uri>().unwrap();
            assert!(
                !is_install_file(&uri),
                "Should not detect as install file: {}",
                path
            );
        }
    }
}
