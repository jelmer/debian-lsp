//! Code lenses for debian/copyright files.
//!
//! Shows the number of Files paragraphs that reference each standalone
//! License paragraph:
//! - `License: MIT` ->"used by 3 Files paragraphs"
//!
//! When a source root is provided, also shows how many actual files in
//! the source tree match each `Files:` paragraph's glob patterns:
//! - `Files: src/*` ->"12 files"
//! - `Files: docs/legacy/*` ->"0 files"

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use debian_copyright::GlobPattern;
use rowan::ast::AstNode;
use tokio::sync::Mutex;
use tower_lsp_server::ls_types::{CodeLens, Command};

use crate::position::text_range_to_lsp_range;

/// How long cached git file lists remain valid.
const GIT_FILE_LIST_TTL: Duration = Duration::from_secs(300);

/// Cached state for file-count code lenses.
pub(crate) struct CachedFileCounts {
    /// The git-tracked files at the time of caching.
    git_files: Vec<String>,
    /// When the git file list was fetched.
    git_fetched_at: Instant,
    /// The patterns (Files: + Files-Excluded:) that produced the cached counts.
    /// Used to detect when the copyright file changes and we need to recompute.
    pattern_key: Vec<String>,
    /// The computed file-count lenses.
    lenses: Vec<CodeLens>,
}

/// Shared, per-root cache of file-count code lenses.
pub type SharedGitFileCache = Arc<Mutex<HashMap<PathBuf, CachedFileCounts>>>;

/// Create a new shared git file cache.
pub fn new_shared_git_file_cache() -> SharedGitFileCache {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Build a cache key from the file patterns and excluded patterns.
fn build_pattern_key(file_lens_data: &FileLensData) -> Vec<String> {
    let mut key: Vec<String> = file_lens_data
        .excluded_patterns_raw
        .iter()
        .map(|p| format!("X:{p}"))
        .collect();
    for para in &file_lens_data.paragraphs {
        for p in &para.patterns_raw {
            key.push(format!("F:{p}"));
        }
        // Separator between paragraphs
        key.push("|".to_string());
    }
    key
}

/// Get file-count lenses, using cached results when the patterns and
/// git file list haven't changed.
async fn get_file_count_lenses(
    cache: &SharedGitFileCache,
    root: &Path,
    file_lens_data: &FileLensData,
) -> Option<Vec<CodeLens>> {
    let pattern_key = build_pattern_key(file_lens_data);

    // Check cache: reuse if patterns match and git file list is fresh.
    {
        let map = cache.lock().await;
        if let Some(entry) = map.get(root) {
            if entry.pattern_key == pattern_key
                && entry.git_fetched_at.elapsed() < GIT_FILE_LIST_TTL
            {
                return Some(entry.lenses.clone());
            }
        }
    }

    // Fetch git file list (possibly reusing a still-fresh cached list).
    let git_files = {
        let map = cache.lock().await;
        map.get(root)
            .filter(|e| e.git_fetched_at.elapsed() < GIT_FILE_LIST_TTL)
            .map(|e| e.git_files.clone())
    };

    let git_files = match git_files {
        Some(files) => files,
        None => {
            let root_buf = root.to_path_buf();
            tokio::task::spawn_blocking(move || git_ls_files(&root_buf))
                .await
                .ok()
                .flatten()?
        }
    };

    // Compute file counts.
    let included_files: Vec<&str> = git_files
        .iter()
        .map(|s| s.as_str())
        .filter(|f| {
            !file_lens_data
                .excluded_patterns
                .iter()
                .any(|p| p.is_match(f))
        })
        .collect();

    let paragraphs = &file_lens_data.paragraphs;
    let mut counts = vec![0usize; paragraphs.len()];
    for f in &included_files {
        let winning_idx = paragraphs
            .iter()
            .rposition(|para| para.patterns.iter().any(|p| p.is_match(f)));
        if let Some(idx) = winning_idx {
            counts[idx] += 1;
        }
    }

    let lenses: Vec<CodeLens> = paragraphs
        .iter()
        .zip(&counts)
        .map(|(para_data, &count)| {
            let title = match count {
                1 => "1 file".to_string(),
                n => format!("{n} files"),
            };
            CodeLens {
                range: para_data.range,
                command: Some(Command {
                    title,
                    command: "debian-lsp.noop".to_string(),
                    arguments: None,
                }),
                data: None,
            }
        })
        .collect();

    // Store in cache.
    {
        let mut map = cache.lock().await;
        map.insert(
            root.to_path_buf(),
            CachedFileCounts {
                git_files,
                git_fetched_at: Instant::now(),
                pattern_key,
                lenses: lenses.clone(),
            },
        );
    }

    Some(lenses)
}

/// List git-tracked files relative to the given working directory.
fn git_ls_files(root: &Path) -> Option<Vec<String>> {
    let output = match std::process::Command::new("git")
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
            .filter_map(|s| std::str::from_utf8(s).ok().map(|s| s.to_string()))
            .collect(),
    )
}

/// Generate code lenses for copyright license and files paragraphs.
///
/// For each standalone License paragraph, counts how many Files paragraphs
/// reference that license name and displays the count as a code lens.
///
/// When `source_root` is provided, also adds a code lens to each Files
/// paragraph showing how many actual files in the source tree match
/// the paragraph's glob patterns. Files listed in the header's
/// `Files-Excluded` field are excluded from counts. The file listing
/// is obtained from `git ls-files` and offloaded to a blocking thread.
pub async fn generate_code_lenses(
    parsed: &debian_copyright::lossless::Parse,
    source_text: &str,
    source_root: Option<&Path>,
    git_file_cache: &SharedGitFileCache,
) -> Vec<CodeLens> {
    // Extract everything we need from the non-Send parsed tree up front,
    // before any .await points.
    let mut lenses = generate_license_lenses(parsed, source_text);
    let file_lens_data = extract_file_lens_data(parsed, source_text);

    if file_lens_data.is_empty() || source_root.is_none() {
        return lenses;
    }

    // Get file-count lenses (cached when patterns and git file list unchanged).
    if let Some(mut file_lenses) =
        get_file_count_lenses(git_file_cache, source_root.unwrap(), &file_lens_data).await
    {
        file_lenses.append(&mut lenses);
        file_lenses
    } else {
        lenses
    }
}

/// Pre-extracted data for a single Files paragraph.
struct FilesParagraphData {
    patterns_raw: Vec<String>,
    patterns: Vec<GlobPattern>,
    range: tower_lsp_server::ls_types::Range,
}

/// Pre-extracted data needed for file-count lenses.
struct FileLensData {
    excluded_patterns_raw: Vec<String>,
    excluded_patterns: Vec<GlobPattern>,
    paragraphs: Vec<FilesParagraphData>,
}

impl FileLensData {
    fn is_empty(&self) -> bool {
        self.paragraphs.is_empty()
    }
}

/// Extract file lens data from the parsed copyright tree (synchronous,
/// no Send requirement).
fn extract_file_lens_data(
    parsed: &debian_copyright::lossless::Parse,
    source_text: &str,
) -> FileLensData {
    let copyright = parsed.to_copyright();

    let excluded_patterns_raw: Vec<String> = copyright
        .header()
        .and_then(|h| h.files_excluded())
        .unwrap_or_default();
    let excluded_patterns: Vec<GlobPattern> = excluded_patterns_raw
        .iter()
        .map(|p| GlobPattern::new(p))
        .collect();

    let mut paragraphs = Vec::new();
    for files_para in copyright.iter_files() {
        let para = files_para.as_deb822();
        let para_range = para.syntax().text_range();

        let patterns_raw = files_para.files();
        let patterns: Vec<GlobPattern> = patterns_raw.iter().map(|p| GlobPattern::new(p)).collect();

        let range = if let Some(entry) = para
            .entries()
            .find(|e| e.key().is_some_and(|k| k.eq_ignore_ascii_case("Files")))
        {
            text_range_to_lsp_range(source_text, entry.text_range())
        } else {
            text_range_to_lsp_range(source_text, para_range)
        };

        paragraphs.push(FilesParagraphData {
            patterns_raw,
            patterns,
            range,
        });
    }

    FileLensData {
        excluded_patterns_raw,
        excluded_patterns,
        paragraphs,
    }
}

/// Generate license-usage code lenses only (no source root needed).
pub fn generate_license_lenses(
    parsed: &debian_copyright::lossless::Parse,
    source_text: &str,
) -> Vec<CodeLens> {
    let copyright = parsed.to_copyright();
    let mut lenses = Vec::new();

    let mut license_usage: HashMap<String, usize> = HashMap::new();
    for files_para in copyright.iter_files() {
        if let Some(license) = files_para.license() {
            if let Some(expr) = license.expr() {
                for name in expr.license_names() {
                    *license_usage.entry(name.to_lowercase()).or_insert(0) += 1;
                }
            }
        }
    }

    for license_para in copyright.iter_licenses() {
        let para = license_para.as_deb822();
        let para_range = para.syntax().text_range();

        let Some(name) = license_para.name() else {
            continue;
        };

        let key = name.to_lowercase();
        let count = license_usage.get(&key).copied().unwrap_or(0);

        let title = match count {
            0 => "unused".to_string(),
            1 => "used by 1 Files paragraph".to_string(),
            n => format!("used by {n} Files paragraphs"),
        };

        let entry_range = if let Some(entry) = para
            .entries()
            .find(|e| e.key().is_some_and(|k| k.eq_ignore_ascii_case("License")))
        {
            text_range_to_lsp_range(source_text, entry.text_range())
        } else {
            text_range_to_lsp_range(source_text, para_range)
        };

        lenses.push(CodeLens {
            range: entry_range,
            command: Some(Command {
                title,
                command: "debian-lsp.noop".to_string(),
                arguments: None,
            }),
            data: None,
        });
    }

    lenses
}

#[cfg(test)]
mod tests {
    use super::*;
    use debian_copyright::lossless::Parse;

    fn parse(text: &str) -> Parse {
        Parse::parse_relaxed(text)
    }

    #[test]
    fn test_license_used_by_multiple_files() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/*
Copyright: 2024 Alice
License: MIT

Files: lib/*
Copyright: 2024 Bob
License: MIT

Files: debian/*
Copyright: 2024 Carol
License: GPL-2+

License: MIT
 Permission is hereby granted...

License: GPL-2+
 This program is free software...
";
        let parsed = parse(text);
        let lenses = generate_license_lenses(&parsed, text);

        assert_eq!(lenses.len(), 2);
        assert_eq!(
            lenses[0].command.as_ref().unwrap().title,
            "used by 2 Files paragraphs"
        );
        assert_eq!(
            lenses[1].command.as_ref().unwrap().title,
            "used by 1 Files paragraph"
        );
    }

    #[test]
    fn test_unused_license() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Test
License: MIT

License: MIT
 Permission is hereby granted...

License: Apache-2.0
 Licensed under the Apache License...
";
        let parsed = parse(text);
        let lenses = generate_license_lenses(&parsed, text);

        assert_eq!(lenses.len(), 2);
        assert_eq!(
            lenses[0].command.as_ref().unwrap().title,
            "used by 1 Files paragraph"
        );
        assert_eq!(lenses[1].command.as_ref().unwrap().title, "unused");
    }

    #[test]
    fn test_no_standalone_licenses() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Test
License: MIT
 Permission is hereby granted...
";
        let parsed = parse(text);
        let lenses = generate_license_lenses(&parsed, text);

        assert_eq!(lenses.len(), 0);
    }

    #[test]
    fn test_case_insensitive_matching() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Test
License: mit

License: MIT
 Permission is hereby granted...
";
        let parsed = parse(text);
        let lenses = generate_license_lenses(&parsed, text);

        assert_eq!(lenses.len(), 1);
        assert_eq!(
            lenses[0].command.as_ref().unwrap().title,
            "used by 1 Files paragraph"
        );
    }

    #[test]
    fn test_empty_copyright() {
        let text = "";
        let parsed = parse(text);
        let lenses = generate_license_lenses(&parsed, text);

        assert_eq!(lenses.len(), 0);
    }

    #[test]
    fn test_or_expression_counts_individual_licenses() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/*
Copyright: 2024 Alice
License: GPL-2+ or MIT

License: GPL-2+
 This program is free software...

License: MIT
 Permission is hereby granted...
";
        let parsed = parse(text);
        let lenses = generate_license_lenses(&parsed, text);

        assert_eq!(lenses.len(), 2);
        assert_eq!(
            lenses[0].command.as_ref().unwrap().title,
            "used by 1 Files paragraph"
        );
        assert_eq!(
            lenses[1].command.as_ref().unwrap().title,
            "used by 1 Files paragraph"
        );
    }

    #[tokio::test]
    async fn test_file_count_with_source_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Initialize a git repo with some files
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(root)
            .output()
            .unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "").unwrap();
        std::fs::write(root.join("src/lib.rs"), "").unwrap();
        std::fs::create_dir_all(root.join("debian")).unwrap();
        std::fs::write(root.join("debian/rules"), "").unwrap();
        std::fs::write(root.join("README"), "").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();

        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Carol
License: MIT

Files: src/*
Copyright: 2024 Alice
License: MIT

Files: debian/*
Copyright: 2024 Bob
License: GPL-2+
";
        let parsed = parse(text);
        let lenses =
            generate_code_lenses(&parsed, text, Some(root), &new_shared_git_file_cache()).await;

        // 3 file-count lenses + 0 license lenses (no standalone License paragraphs)
        // Last matching stanza wins: src/* claims src/{main,lib}.rs,
        // debian/* claims debian/rules, * only counts README.
        assert_eq!(lenses.len(), 3);
        assert_eq!(lenses[0].command.as_ref().unwrap().title, "1 file");
        assert_eq!(lenses[1].command.as_ref().unwrap().title, "2 files");
        assert_eq!(lenses[2].command.as_ref().unwrap().title, "1 file");
    }

    #[tokio::test]
    async fn test_file_count_excludes_files_excluded() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::process::Command::new("git")
            .args(["init"])
            .current_dir(root)
            .output()
            .unwrap();
        std::fs::write(root.join("foo.c"), "").unwrap();
        std::fs::write(root.join("vendor.js"), "").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();

        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Files-Excluded: vendor.js

Files: *
Copyright: 2024 Test
License: MIT
";
        let parsed = parse(text);
        let lenses =
            generate_code_lenses(&parsed, text, Some(root), &new_shared_git_file_cache()).await;

        assert_eq!(lenses.len(), 1);
        assert_eq!(lenses[0].command.as_ref().unwrap().title, "1 file");
    }

    #[tokio::test]
    async fn test_file_count_no_source_root() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: *
Copyright: 2024 Test
License: MIT

License: MIT
 text
";
        let parsed = parse(text);
        let lenses = generate_code_lenses(&parsed, text, None, &new_shared_git_file_cache()).await;

        // Only license lenses, no file-count lenses
        assert_eq!(lenses.len(), 1);
        assert_eq!(
            lenses[0].command.as_ref().unwrap().title,
            "used by 1 Files paragraph"
        );
    }
}
