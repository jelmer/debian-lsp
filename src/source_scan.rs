use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind};

/// List git-tracked files relative to `root`.
pub(crate) fn git_ls_files(root: &Path) -> Option<Vec<String>> {
    let output = match Command::new("git")
        .arg("ls-files")
        .arg("-z")
        .current_dir(root)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("failed to run git ls-files in {}: {e}", root.display());
            return None;
        }
    };
    if !output.status.success() {
        tracing::info!(
            "git ls-files failed in {} (not a git repo?)",
            root.display()
        );
        return None;
    }
    Some(
        output
            .stdout
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .filter_map(|s| std::str::from_utf8(s).ok().map(str::to_string))
            .collect(),
    )
}

/// Source candidates for a path token, from the files tracked under `root`.
pub fn source_candidates(root: &Path, prefix: &str) -> Vec<CompletionItem> {
    let mut found: BTreeMap<String, bool> = BTreeMap::new();
    record_tracked(root, prefix, &mut found);
    shape(found)
}

/// Record git-tracked files under `root` at the current directory level.
pub(crate) fn record_tracked(root: &Path, prefix: &str, out: &mut BTreeMap<String, bool>) {
    let dir = dir_prefix(prefix);
    if let Some(files) = git_ls_files(root) {
        for file in &files {
            record(file, dir, prefix, out);
        }
    }
}

/// The directory portion of `prefix`, up to and including the last slash.
pub(crate) fn dir_prefix(prefix: &str) -> &str {
    match prefix.rfind('/') {
        Some(i) => &prefix[..=i],
        None => "",
    }
}

/// Turn the collected path -> is_dir map into completion items.
pub(crate) fn shape(found: BTreeMap<String, bool>) -> Vec<CompletionItem> {
    found
        .into_iter()
        .map(|(path, is_dir)| {
            let (label, kind) = if is_dir {
                (format!("{path}/"), CompletionItemKind::FOLDER)
            } else {
                (path, CompletionItemKind::FILE)
            };
            CompletionItem {
                label,
                kind: Some(kind),
                ..Default::default()
            }
        })
        .collect()
}

/// Offer `file`'s entry at the current directory level `dir`: either the
/// file itself, or the subdirectory that leads to it.
fn record(file: &str, dir: &str, prefix: &str, out: &mut BTreeMap<String, bool>) {
    let Some(rest) = file.strip_prefix(dir) else {
        return;
    };
    let (candidate, is_dir) = match rest.find('/') {
        Some(i) => (&file[..dir.len() + i], true),
        None => (file, false),
    };
    if candidate.starts_with(prefix) {
        out.insert(candidate.to_string(), is_dir);
    }
}

#[cfg(test)]
pub(crate) fn git_tree(tracked: &[&str], untracked: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    run_git(root, &["init"]);
    for rel in tracked.iter().chain(untracked) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "").unwrap();
    }
    for rel in tracked {
        run_git(root, &["add", rel]);
    }
    dir
}

#[cfg(test)]
fn run_git(root: &Path, args: &[&str]) {
    Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(items: &[CompletionItem]) -> Vec<String> {
        items.iter().map(|i| i.label.clone()).collect()
    }

    #[test]
    fn offers_files_from_the_tree() {
        let dir = git_tree(&["src/main.rs", "README"], &[]);
        let labels = labels(&source_candidates(dir.path(), ""));
        assert!(labels.contains(&"README".to_string()));
        assert!(labels.contains(&"src/".to_string()));
    }

    #[test]
    fn includes_files_under_debian() {
        let dir = git_tree(&["debian/foo.1", "debian/control"], &[]);
        let labels = labels(&source_candidates(dir.path(), "debian/"));
        assert!(labels.contains(&"debian/foo.1".to_string()));
        assert!(labels.contains(&"debian/control".to_string()));
    }

    #[test]
    fn offers_only_the_current_directory_level() {
        let dir = git_tree(&["usr/bin/foo", "usr/lib/bar"], &[]);
        let top = labels(&source_candidates(dir.path(), ""));
        assert_eq!(top, vec!["usr/".to_string()]);
        let under_usr = labels(&source_candidates(dir.path(), "usr/"));
        assert_eq!(
            under_usr,
            vec!["usr/bin/".to_string(), "usr/lib/".to_string()]
        );
    }

    #[test]
    fn filters_by_prefix() {
        let dir = git_tree(&["usr/bin/foo", "usr/lib/bar", "etc/conf"], &[]);
        let labels = labels(&source_candidates(dir.path(), "usr/"));
        assert!(labels.iter().all(|l| l.starts_with("usr/")));
        assert!(!labels.iter().any(|l| l.starts_with("etc")));
    }

    #[test]
    fn directories_end_with_a_slash() {
        let dir = git_tree(&["src/main.rs"], &[]);
        let items = source_candidates(dir.path(), "src");
        let src = items.iter().find(|i| i.label == "src/").unwrap();
        assert_eq!(src.kind, Some(CompletionItemKind::FOLDER));
    }

    #[test]
    fn no_match_is_empty() {
        let dir = git_tree(&["README"], &[]);
        assert!(source_candidates(dir.path(), "usr/").is_empty());
    }
}
