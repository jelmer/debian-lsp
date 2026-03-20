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
use std::path::Path;

use debian_copyright::GlobPattern;
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{CodeLens, Command};

use crate::position::text_range_to_lsp_range;

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
) -> Vec<CodeLens> {
    // Extract everything we need from the non-Send parsed tree up front,
    // before any .await points.
    let mut lenses = generate_license_lenses(parsed, source_text);
    let file_lens_data = extract_file_lens_data(parsed, source_text);

    if file_lens_data.is_empty() || source_root.is_none() {
        return lenses;
    }

    // Obtain git-tracked files in a blocking task.
    let root = source_root.unwrap().to_path_buf();
    let source_files = tokio::task::spawn_blocking(move || git_ls_files(&root))
        .await
        .ok()
        .flatten();

    let Some(source_files) = source_files else {
        return lenses;
    };

    // Filter out excluded files
    let included_files: Vec<&str> = source_files
        .iter()
        .map(|s| s.as_str())
        .filter(|f| {
            !file_lens_data
                .excluded_patterns
                .iter()
                .any(|p| p.is_match(f))
        })
        .collect();

    // Insert file-count lenses before the license lenses
    let mut file_lenses = Vec::new();
    for para_data in &file_lens_data.paragraphs {
        let count = included_files
            .iter()
            .filter(|f| para_data.patterns.iter().any(|p| p.is_match(f)))
            .count();

        let title = match count {
            1 => "1 file".to_string(),
            n => format!("{n} files"),
        };

        file_lenses.push(CodeLens {
            range: para_data.range,
            command: Some(Command {
                title,
                command: "debian-lsp.noop".to_string(),
                arguments: None,
            }),
            data: None,
        });
    }

    file_lenses.append(&mut lenses);
    file_lenses
}

/// Pre-extracted data for a single Files paragraph.
struct FilesParagraphData {
    patterns: Vec<GlobPattern>,
    range: tower_lsp_server::ls_types::Range,
}

/// Pre-extracted data needed for file-count lenses.
struct FileLensData {
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

    let excluded_patterns: Vec<GlobPattern> = copyright
        .header()
        .and_then(|h| h.files_excluded())
        .unwrap_or_default()
        .iter()
        .map(|p| GlobPattern::new(p))
        .collect();

    let mut paragraphs = Vec::new();
    for files_para in copyright.iter_files() {
        let para = files_para.as_deb822();
        let para_range = para.syntax().text_range();

        let patterns: Vec<GlobPattern> = files_para
            .files()
            .iter()
            .map(|p| GlobPattern::new(p))
            .collect();

        let range = if let Some(entry) = para
            .entries()
            .find(|e| e.key().is_some_and(|k| k.eq_ignore_ascii_case("Files")))
        {
            text_range_to_lsp_range(source_text, entry.text_range())
        } else {
            text_range_to_lsp_range(source_text, para_range)
        };

        paragraphs.push(FilesParagraphData { patterns, range });
    }

    FileLensData {
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

Files: src/*
Copyright: 2024 Alice
License: MIT

Files: debian/*
Copyright: 2024 Bob
License: GPL-2+

Files: *
Copyright: 2024 Carol
License: MIT
";
        let parsed = parse(text);
        let lenses = generate_code_lenses(&parsed, text, Some(root)).await;

        // 3 file-count lenses + 0 license lenses (no standalone License paragraphs)
        assert_eq!(lenses.len(), 3);
        assert_eq!(lenses[0].command.as_ref().unwrap().title, "2 files");
        assert_eq!(lenses[1].command.as_ref().unwrap().title, "1 file");
        assert_eq!(lenses[2].command.as_ref().unwrap().title, "4 files");
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
        let lenses = generate_code_lenses(&parsed, text, Some(root)).await;

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
        let lenses = generate_code_lenses(&parsed, text, None).await;

        // Only license lenses, no file-count lenses
        assert_eq!(lenses.len(), 1);
        assert_eq!(
            lenses[0].command.as_ref().unwrap().title,
            "used by 1 Files paragraph"
        );
    }
}
