use super::detection::list_patch_files;
use std::collections::HashSet;

use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position, Uri};

/// Get completion items for a debian/patches/series file
pub fn get_completions(
    uri: &Uri,
    parsed: &patchkit::edit::Parse<patchkit::edit::series::lossless::SeriesFile>,
    source_text: &str,
    position: Position,
) -> Vec<CompletionItem> {
    let series = parsed.tree();

    let current_line = source_text
        .lines()
        .nth(position.line as usize)
        .unwrap_or("");
    let before_cursor = &current_line[..position.character as usize];
    let tokens: Vec<&str> = before_cursor.split_whitespace().collect();

    if tokens.len() >= 2 {
        return Vec::new();
    }

    if tokens.len() == 1 && before_cursor.ends_with(' ') {
        return get_strip_option_completions(tokens[0]);
    }

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

/// Get completion items for the strip option (-p0, -p1, -p2...)
fn get_strip_option_completions(patch: &str) -> Vec<CompletionItem> {
    let count = patch.split('/').count() - 1;
    (0..=count)
        .map(|n| {
            let label = format!("-p{}", n);
            CompletionItem {
                label: label.clone(),
                kind: Some(CompletionItemKind::VALUE),
                detail: Some(format!("Strip {} path segment(s)", n)),
                ..Default::default()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_series(
        patches: &[&str],
    ) -> patchkit::edit::Parse<patchkit::edit::series::lossless::SeriesFile> {
        let text = patches.join("\n") + "\n";
        patchkit::edit::series::parse(&text)
    }

    fn empty_series() -> patchkit::edit::Parse<patchkit::edit::series::lossless::SeriesFile> {
        patchkit::edit::series::parse("")
    }

    #[test]
    fn test_strip_option_completions_no_slash() {
        let completions = get_strip_option_completions("fix-arm.patch");
        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["-p0"]);
        assert_eq!(completions.len(), 1);
    }

    #[test]
    fn test_strip_option_completions_one_slash() {
        let completions = get_strip_option_completions("upstream/fix-arm.patch");
        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["-p0", "-p1"]);
        assert_eq!(completions.len(), 2);
    }

    #[test]
    fn test_strip_option_completions_two_slashes() {
        let completions = get_strip_option_completions("a/b/fix.patch");
        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["-p0", "-p1", "-p2"]);
        assert_eq!(completions.len(), 3);
    }

    #[test]
    fn test_strip_option_completions_have_details() {
        let completions = get_strip_option_completions("fix-arm.patch");
        for c in &completions {
            assert!(c.detail.is_some());
        }
    }

    #[test]
    fn test_strip_option_completions_detail_format() {
        let completions = get_strip_option_completions("upstream/fix-arm.patch");
        for (i, c) in completions.iter().enumerate() {
            assert_eq!(
                c.detail.as_deref(),
                Some(format!("Strip {} path segment(s)", i).as_str())
            );
        }
    }

    #[test]
    fn test_strip_option_completions_kind() {
        let completions = get_strip_option_completions("fix-arm.patch");
        assert!(completions
            .iter()
            .all(|c| c.kind == Some(CompletionItemKind::VALUE)));
    }

    #[test]
    fn test_strip_options_after_space() {
        let parsed = make_series(&["fix-arm.patch"]);
        let source_text = "fix-arm.patch ";
        let position = Position::new(0, 14);
        let uri: Uri = "file:///debian/patches/series".parse().unwrap();

        let completions = get_completions(&uri, &parsed, source_text, position);
        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels, vec!["-p0"]);
        assert!(completions
            .iter()
            .all(|c| c.kind == Some(CompletionItemKind::VALUE)));
    }

    #[test]
    fn test_strip_options_after_space_subdir() {
        let parsed = make_series(&["upstream/fix-arm.patch"]);
        let source_text = "upstream/fix-arm.patch ";
        let position = Position::new(0, 23);
        let uri: Uri = "file:///debian/patches/series".parse().unwrap();

        let completions = get_completions(&uri, &parsed, source_text, position);
        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels, vec!["-p0", "-p1"]);
    }

    #[test]
    fn test_no_completions_when_two_tokens() {
        let parsed = make_series(&["fix-arm.patch"]);
        let source_text = "fix-arm.patch -p1";
        let position = Position::new(0, 17);
        let uri: Uri = "file:///debian/patches/series".parse().unwrap();

        let completions = get_completions(&uri, &parsed, source_text, position);
        assert!(completions.is_empty());
    }

    #[test]
    fn test_no_completions_when_patch_and_option_present() {
        let parsed = make_series(&["fix-arm.patch"]);
        let source_text = "fix-arm.patch -p1 ";
        let position = Position::new(0, 18);
        let uri: Uri = "file:///debian/patches/series".parse().unwrap();

        let completions = get_completions(&uri, &parsed, source_text, position);
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

        let already_listed: HashSet<String> =
            vec!["fix-arm.patch".to_string()].into_iter().collect();

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

        let already_listed: HashSet<String> = HashSet::new();

        let completions = get_patch_file_completions(&patch_files, &already_listed);
        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();

        assert_eq!(labels, vec!["aaa.patch", "mmm.patch", "zzz.patch"]);
    }

    #[test]
    fn test_patch_file_completions_have_file_kind() {
        let patch_files: HashSet<String> = vec!["fix-arm.patch".to_string()].into_iter().collect();

        let already_listed: HashSet<String> = HashSet::new();

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

        let already_listed: HashSet<String> =
            vec!["fix-arm.patch".to_string(), "fix-mips.patch".to_string()]
                .into_iter()
                .collect();

        let completions = get_patch_file_completions(&patch_files, &already_listed);
        assert!(completions.is_empty());
    }

    #[test]
    fn test_no_strip_options_without_space() {
        let parsed = empty_series();
        let source_text = "fix-arm";
        let position = Position::new(0, 7);
        let uri: Uri = "file:///debian/patches/series".parse().unwrap();

        let completions = get_completions(&uri, &parsed, source_text, position);
        let labels: Vec<&str> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(!labels.contains(&"-p0"));
        assert!(!labels.contains(&"-p1"));
        assert!(!labels.contains(&"-p2"));
    }
}
