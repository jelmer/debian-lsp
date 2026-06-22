//! Resolving repo-relative file references to navigable links.
//!
//! Shared by the `debian/copyright` document-link and SCIP indexers and the
//! changelog document-link support: both turn a path mentioned in a packaging
//! file into a link only when it resolves to a real file inside the package
//! tree.

use std::path::{Component, Path, PathBuf};
use tower_lsp_server::ls_types::Uri;

/// Resolve the package root (the directory containing `debian/`) from the URI of
/// a file of the form `.../debian/<name>` (e.g. `debian/changelog`,
/// `debian/copyright`).
///
/// Returns `None` for a non-file URI or one too shallow to have a `debian/`
/// parent.
pub fn source_root(uri: &Uri) -> Option<PathBuf> {
    let path = uri.to_file_path()?;
    // `<root>/debian/<name>` -> `<root>`.
    Some(path.parent()?.parent()?.to_path_buf())
}

/// Resolve a DEP-5 `Files:` pattern to a repo-relative path that can be linked.
///
/// Returns the path (relative to `root`) when the pattern names a single
/// existing file inside the tree, or `None` when the pattern is a glob, escapes
/// the tree (absolute path or `..`), or does not resolve to a regular file.
///
/// The returned string is the pattern's literal form with DEP-5 escapes
/// decoded, so `\*.txt` resolves to the file `*.txt`.
pub fn file_link_target(root: &Path, pattern: &str) -> Option<String> {
    // A glob matches many files and can't resolve to one.
    let rel = debian_copyright::glob::literal_path(pattern)?;
    let rel_path = Path::new(&rel);
    // Stay inside the package tree: reject absolute paths and `..` traversal.
    if rel_path.components().any(|c| {
        matches!(
            c,
            Component::RootDir | Component::Prefix(_) | Component::ParentDir
        )
    }) {
        return None;
    }
    if !root.join(rel_path).is_file() {
        return None;
    }
    Some(rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (rel, content) in files {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, content).unwrap();
        }
        dir
    }

    #[test]
    fn source_root_strips_debian_file() {
        let uri = Uri::from_file_path("/home/x/pkg/debian/copyright").unwrap();
        assert_eq!(source_root(&uri).unwrap(), Path::new("/home/x/pkg"));
    }

    #[test]
    fn source_root_none_for_non_file_uri() {
        let uri: Uri = "untitled:Untitled-1".parse().unwrap();
        assert_eq!(source_root(&uri), None);
    }

    #[test]
    fn links_existing_literal_path() {
        let dir = write_tree(&[("src/main.c", "x")]);
        assert_eq!(
            file_link_target(dir.path(), "src/main.c").as_deref(),
            Some("src/main.c")
        );
    }

    #[test]
    fn rejects_glob() {
        let dir = write_tree(&[("src/main.c", "x")]);
        assert_eq!(file_link_target(dir.path(), "src/*"), None);
        assert_eq!(file_link_target(dir.path(), "vendor/*"), None);
    }

    #[test]
    fn rejects_missing_file() {
        let dir = write_tree(&[]);
        assert_eq!(file_link_target(dir.path(), "src/absent.c"), None);
    }

    #[test]
    fn rejects_directory() {
        let dir = write_tree(&[("src/main.c", "x")]);
        // `src` is a directory, not a regular file.
        assert_eq!(file_link_target(dir.path(), "src"), None);
    }

    #[test]
    fn rejects_path_escape() {
        // Even if the target exists, an absolute path or `..` must not link
        // outside the package tree.
        let dir = write_tree(&[("inside.txt", "x")]);
        assert_eq!(file_link_target(dir.path(), "/etc/passwd"), None);
        assert_eq!(file_link_target(dir.path(), "../inside.txt"), None);
    }

    #[test]
    fn decodes_escaped_literal() {
        let dir = write_tree(&[("*.txt", "x")]);
        // `\*.txt` is an escaped literal naming the file `*.txt`, not a glob.
        assert_eq!(
            file_link_target(dir.path(), r"\*.txt").as_deref(),
            Some("*.txt")
        );
    }
}
