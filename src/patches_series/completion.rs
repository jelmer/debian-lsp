use super::detection::list_patch_files;
use std::collections::HashSet;

use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Uri};

/// Get completion items for a debian/patches/series file
pub fn get_completions(
    uri: &Uri,
    parsed: &patchkit::edit::Parse<patchkit::edit::series::lossless::SeriesFile>,
) -> Vec<CompletionItem> {
    let series = parsed.tree();

    let already_listed: HashSet<String> = series.patch_entries().filter_map(|e| e.name()).collect();
    let patch_files = list_patch_files(uri);
    get_patch_file_completions(&patch_files, &already_listed)
}

// Get snippet completions for each patches in the debian/patches folder
fn get_patch_file_completions(
    patch_files: &HashSet<String>,
    already_listed: &HashSet<String>,
) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = patch_files
        .iter()
        .filter(|name| !already_listed.contains(*name))
        .map(|name| CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::FILE),
            detail: Some("Patch file".to_string()),
            ..Default::default()
        })
        .collect();

    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use patchkit::quilt::{Series, SeriesEntry};

    fn make_series(patches: &[&str]) -> Series {
        Series {
            entries: patches
                .iter()
                .map(|name| SeriesEntry::Patch {
                    name: name.to_string(),
                    options: vec![],
                })
                .collect(),
        }
    }

    fn empty_series() -> Series {
        Series { entries: vec![] }
    }

    #[test]
    fn test_no_completions_when_two_tokens() {
        let series = make_series(&["fix-arm.patch"]);
        let source_text = "fix-arm.patch -p1";
        let position = Position::new(0, 17);
        let uri: Uri = "file:///debian/patches/series".parse().unwrap();

        let completions = get_completions(&uri, &series, source_text, position);
        assert!(completions.is_empty());
    }

    #[test]
    fn test_no_completions_when_patch_and_option_present() {
        let series = make_series(&["fix-arm.patch"]);
        let source_text = "fix-arm.patch -p1 ";
        let position = Position::new(0, 18);
        let uri: Uri = "file:///debian/patches/series".parse().unwrap();

        let completions = get_completions(&uri, &series, source_text, position);
        assert!(completions.is_empty());
    }

    #[test]
    fn test_patch_file_completions_excludes_already_listed() {
        let patch_files: HashSet<String> = vec![
            "fix-arm.patch".to_string(),
            "fix-mips.patch".to_string(),
            "CVE-2024.patch".to_string(),
        ]
        .into_iter()
        .collect();

        let already_listed: HashSet<&str> = vec!["fix-arm.patch"].into_iter().collect();

        let completions = get_patch_file_completions(&patch_files, &already_listed);
        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(!labels.contains(&"fix-arm.patch"));
        assert!(labels.contains(&"fix-mips.patch"));
        assert!(labels.contains(&"CVE-2024.patch"));
    }

    #[test]
    fn test_patch_file_completions_sorted() {
        let patch_files: HashSet<String> = vec![
            "zzz.patch".to_string(),
            "aaa.patch".to_string(),
            "mmm.patch".to_string(),
        ]
        .into_iter()
        .collect();

        let already_listed: HashSet<&str> = HashSet::new();

        let completions = get_patch_file_completions(&patch_files, &already_listed);
        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels, vec!["aaa.patch", "mmm.patch", "zzz.patch"]);
    }

    #[test]
    fn test_patch_file_completions_have_file_kind() {
        let patch_files: HashSet<String> = vec!["fix-arm.patch".to_string()].into_iter().collect();

        let already_listed: HashSet<&str> = HashSet::new();

        let completions = get_patch_file_completions(&patch_files, &already_listed);

        assert!(completions
            .iter()
            .all(|c| c.kind == Some(CompletionItemKind::FILE)));
    }

    #[test]
    fn test_patch_file_completions_all_listed_returns_empty() {
        let patch_files: HashSet<String> =
            vec!["fix-arm.patch".to_string(), "fix-mips.patch".to_string()]
                .into_iter()
                .collect();

        let already_listed: HashSet<&str> = vec!["fix-arm.patch", "fix-mips.patch"]
            .into_iter()
            .collect();

        let completions = get_patch_file_completions(&patch_files, &already_listed);
        assert!(completions.is_empty());
    }
}
