use std::collections::BTreeMap;
use std::path::Path;

use tower_lsp_server::ls_types::CompletionItem;

use crate::source_scan;

/// Install-source candidates for a path token: files tracked in the source
/// tree plus staged build output under debian/tmp.
pub(crate) fn source_candidates(debian_dir: &Path, prefix: &str) -> Vec<CompletionItem> {
    let mut found: BTreeMap<String, bool> = BTreeMap::new();
    if let Some(root) = debian_dir.parent() {
        source_scan::record_tracked(root, prefix, &mut found);
    }
    record_staged(debian_dir, prefix, &mut found);
    source_scan::shape(found)
}

/// Staged build output under debian/tmp for a path token.
pub(crate) fn staged_candidates(debian_dir: &Path, prefix: &str) -> Vec<CompletionItem> {
    let mut found: BTreeMap<String, bool> = BTreeMap::new();
    record_staged(debian_dir, prefix, &mut found);
    source_scan::shape(found)
}

/// Record entries directly under debian/tmp/<dir> that match `prefix`.
// FIXME: debian/tmp is only a convention; a package can install to a
// different directory (e.g. dh_auto_install --destdir).
fn record_staged(debian_dir: &Path, prefix: &str, out: &mut BTreeMap<String, bool>) {
    let dir = source_scan::dir_prefix(prefix);
    if let Ok(entries) = std::fs::read_dir(debian_dir.join("tmp").join(dir)) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let candidate = format!("{dir}{}", name.to_string_lossy());
            if candidate.starts_with(prefix) {
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                out.insert(candidate, is_dir);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_scan::git_tree;

    fn labels(items: &[CompletionItem]) -> Vec<String> {
        items.iter().map(|i| i.label.clone()).collect()
    }

    #[test]
    fn source_candidates_offers_tree_and_debian_tmp() {
        let dir = git_tree(&["README", "debian/tmp/usr/bin/prog"], &[]);
        let debian = dir.path().join("debian");
        let top = labels(&source_candidates(&debian, ""));
        assert!(top.contains(&"README".to_string()));
        assert!(top.contains(&"usr/".to_string()));
    }

    #[test]
    fn staged_candidates_offers_only_debian_tmp() {
        let dir = git_tree(&["README", "debian/tmp/usr/bin/prog"], &[]);
        let debian = dir.path().join("debian");
        assert!(
            labels(&staged_candidates(&debian, "usr/bin/")).contains(&"usr/bin/prog".to_string())
        );
        assert!(!labels(&staged_candidates(&debian, "")).contains(&"README".to_string()));
    }
}
