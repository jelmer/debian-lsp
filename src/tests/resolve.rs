//! Resolving paths relative to a `debian/tests/control` file.
//!
//! Several features (completion, go-to-definition, document links, SCIP
//! indexing) need to find the directory holding autopkgtest scripts and the
//! source-tree root. This module centralises that logic so they agree on where
//! scripts live.

use std::path::{Path, PathBuf};

use tower_lsp_server::ls_types::Uri;

/// Default directory holding autopkgtest scripts, relative to the source root.
pub const DEFAULT_TESTS_DIRECTORY: &str = "debian/tests";

/// The directory holding a paragraph's test scripts, relative to `root`.
///
/// Uses the paragraph's `Tests-Directory:` field when set, otherwise the
/// default `debian/tests`. A missing paragraph yields the default too.
pub fn tests_directory(paragraph: Option<&deb822_lossless::Paragraph>, root: &Path) -> PathBuf {
    paragraph
        .and_then(|p| p.get("Tests-Directory"))
        .map(|v| root.join(v.trim()))
        .unwrap_or_else(|| root.join(DEFAULT_TESTS_DIRECTORY))
}

/// Derive the source-tree root (the directory containing `debian/`) from a
/// `debian/tests/control` URI.
///
/// `<root>/debian/tests/control` -> `<root>`.
pub fn source_root(uri: &Uri) -> Option<PathBuf> {
    let path = uri.to_file_path()?;
    Some(path.parent()?.parent()?.parent()?.to_path_buf())
}
