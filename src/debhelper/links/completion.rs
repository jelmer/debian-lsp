use std::collections::BTreeMap;
use std::path::Path;

use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use crate::debhelper::completion;

/// Completions for a debian/links file at the given cursor position.
pub fn get_completions(
    text: &str,
    position: Position,
    package_dir: Option<&Path>,
) -> Vec<CompletionItem> {
    completion::get_completions(text, position, |_, prefix| match package_dir {
        Some(dir) => package_candidates(dir, prefix),
        None => Vec::new(),
    })
}

/// Paths inside the package staging directory for a link token.
fn package_candidates(package_dir: &Path, prefix: &str) -> Vec<CompletionItem> {
    let dir = match prefix.rfind('/') {
        Some(i) => &prefix[..=i],
        None => "",
    };
    let mut found: BTreeMap<String, bool> = BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir(package_dir.join(dir)) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let candidate = format!("{dir}{}", name.to_string_lossy());
            if candidate.starts_with(prefix) {
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                found.insert(candidate, is_dir);
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn staging(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for rel in files {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
        }
        dir
    }

    fn labels(items: &[CompletionItem]) -> Vec<String> {
        items.iter().map(|i| i.label.clone()).collect()
    }

    #[test]
    fn completes_the_target_token() {
        let pkg = staging(&["usr/bin/prog"]);
        let items = get_completions("usr/\n", Position::new(0, 4), Some(pkg.path()));
        assert!(labels(&items).contains(&"usr/bin/".to_string()));
    }

    #[test]
    fn completes_the_link_token() {
        let pkg = staging(&["usr/bin/prog"]);
        let items = get_completions(
            "usr/share/foo usr/\n",
            Position::new(0, 18),
            Some(pkg.path()),
        );
        assert!(labels(&items).iter().any(|l| l.starts_with("usr/")));
    }

    #[test]
    fn a_later_token_still_completes() {
        let pkg = staging(&["usr/bin/prog"]);
        let items = get_completions("a b usr/\n", Position::new(0, 8), Some(pkg.path()));
        assert!(labels(&items).iter().any(|l| l.starts_with("usr/")));
    }

    #[test]
    fn directories_end_with_a_slash() {
        let pkg = staging(&["usr/bin/prog"]);
        let items = get_completions("usr", Position::new(0, 3), Some(pkg.path()));
        let usr = items.iter().find(|i| i.label == "usr/").unwrap();
        assert_eq!(usr.kind, Some(CompletionItemKind::FOLDER));
    }

    #[test]
    fn nothing_without_a_package_dir() {
        let items = get_completions("usr/\n", Position::new(0, 4), None);
        assert!(items.is_empty());
    }

    #[test]
    fn dollar_offers_substitution_vars() {
        let items = get_completions("usr/lib/$\n", Position::new(0, 9), None);
        assert!(items.iter().any(|i| i.label == "${DEB_HOST_MULTIARCH}"));
    }

    #[test]
    fn no_completion_in_comment() {
        let items = get_completions("# usr/share/foo\n", Position::new(0, 15), None);
        assert!(items.is_empty());
    }
}
