use std::collections::BTreeMap;
use std::path::Path;

use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use crate::debhelper::completion;

/// Completions for a debian/clean file at the given cursor position.
pub fn get_completions(
    text: &str,
    position: Position,
    debian_dir: Option<&Path>,
) -> Vec<CompletionItem> {
    completion::get_completions(text, position, |_, prefix| match debian_dir {
        Some(dir) => build_candidates(dir, prefix),
        None => Vec::new(),
    })
}

/// Build-artifact candidates for a path token.
fn build_candidates(debian_dir: &Path, prefix: &str) -> Vec<CompletionItem> {
    let Some(root) = debian_dir.parent() else {
        return Vec::new();
    };
    let Some(files) = git_untracked(root) else {
        return Vec::new();
    };

    let dir = match prefix.rfind('/') {
        Some(i) => &prefix[..=i],
        None => "",
    };

    let mut found: BTreeMap<String, bool> = BTreeMap::new();
    for file in &files {
        if file.starts_with("debian/") {
            continue;
        }
        let Some(rest) = file.strip_prefix(dir) else {
            continue;
        };
        let (candidate, is_dir) = match rest.find('/') {
            Some(i) => (&file[..dir.len() + i], true),
            None => (file.as_str(), false),
        };
        if candidate.starts_with(prefix) {
            found.insert(candidate.to_string(), is_dir);
        }
    }

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

/// Files under `root` that git does not track.
fn git_untracked(root: &Path) -> Option<Vec<String>> {
    let output = std::process::Command::new("git")
        .arg("ls-files")
        .arg("--others")
        .arg("-z")
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn git_tree(tracked: &[&str], untracked: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(root)
            .output()
            .unwrap();
        for rel in tracked.iter().chain(untracked) {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
        }
        for rel in tracked {
            std::process::Command::new("git")
                .args(["add", rel])
                .current_dir(root)
                .output()
                .unwrap();
        }
        dir
    }

    fn labels(items: &[CompletionItem]) -> Vec<String> {
        items.iter().map(|i| i.label.clone()).collect()
    }

    #[test]
    fn offers_untracked_artifacts() {
        let dir = git_tree(&["src/main.rs"], &["build/out.o"]);
        let items = get_completions("", Position::new(0, 0), Some(&dir.path().join("debian")));
        assert!(labels(&items).contains(&"build/".to_string()));
    }

    #[test]
    fn skips_files_versioned_in_git() {
        let dir = git_tree(&["src/main.rs", "README"], &["build/out.o"]);
        let items = get_completions("", Position::new(0, 0), Some(&dir.path().join("debian")));
        assert!(!labels(&items).contains(&"README".to_string()));
        assert!(!labels(&items).iter().any(|l| l.starts_with("src")));
    }

    #[test]
    fn offers_only_the_current_directory_level() {
        let dir = git_tree(&[], &["build/out.o", "build/sub/deep.o"]);
        let debian = dir.path().join("debian");
        let top = labels(&get_completions("", Position::new(0, 0), Some(&debian)));
        assert_eq!(top, vec!["build/".to_string()]);
        let inside = labels(&get_completions(
            "build/",
            Position::new(0, 6),
            Some(&debian),
        ));
        assert_eq!(
            inside,
            vec!["build/out.o".to_string(), "build/sub/".to_string()]
        );
    }

    #[test]
    fn filters_by_prefix() {
        let dir = git_tree(&[], &["build/out.o", "obj/foo.o"]);
        let items = get_completions(
            "build/",
            Position::new(0, 6),
            Some(&dir.path().join("debian")),
        );
        assert!(labels(&items).iter().all(|l| l.starts_with("build/")));
        assert!(!labels(&items).iter().any(|l| l.starts_with("obj")));
    }

    #[test]
    fn directories_end_with_a_slash() {
        let dir = git_tree(&[], &["build/out.o"]);
        let items = get_completions(
            "build",
            Position::new(0, 5),
            Some(&dir.path().join("debian")),
        );
        let build = items.iter().find(|i| i.label == "build/").unwrap();
        assert_eq!(build.kind, Some(CompletionItemKind::FOLDER));
    }

    #[test]
    fn nothing_outside_a_git_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let items = get_completions("", Position::new(0, 0), Some(&dir.path().join("debian")));
        assert!(items.is_empty());
    }

    #[test]
    fn nothing_without_a_debian_dir() {
        let items = get_completions("build/", Position::new(0, 6), None);
        assert!(items.is_empty());
    }

    #[test]
    fn dollar_offers_substitution_vars() {
        let items = get_completions("$", Position::new(0, 1), None);
        assert!(items.iter().any(|i| i.label == "${DEB_HOST_MULTIARCH}"));
    }

    #[test]
    fn no_completion_in_comment() {
        let items = get_completions("# build/", Position::new(0, 8), None);
        assert!(items.is_empty());
    }
}
