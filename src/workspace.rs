use std::collections::HashMap;

use rowan::ast::AstNode;
use salsa::Setter;
use text_size::TextRange;
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Uri};

/// Information about a field casing issue
#[derive(Debug, Clone)]
pub struct FieldCasingIssue {
    pub field_name: String,
    pub standard_name: String,
    pub field_range: TextRange,
}

/// Information about an UNRELEASED entry that can be marked for upload
#[derive(Debug, Clone)]
pub struct UnreleasedUploadInfo {
    pub unreleased_range: TextRange,
    pub target_distribution: String,
}

#[salsa::input]
#[derive(Debug)]
pub struct SourceFile {
    pub url: Uri,
    pub text: String,
}

// Store the Parse type directly - it's thread-safe now!
#[salsa::tracked]
pub fn parse_control(
    db: &dyn salsa::Database,
    file: SourceFile,
) -> debian_control::lossless::Parse<debian_control::lossless::Control> {
    let text = file.text(db);
    debian_control::lossless::Control::parse(&text)
}

#[salsa::tracked]
pub fn parse_copyright(
    db: &dyn salsa::Database,
    file: SourceFile,
) -> debian_copyright::lossless::Parse {
    let text = file.text(db);
    debian_copyright::lossless::Parse::parse_relaxed(&text)
}

#[salsa::tracked]
pub fn parse_watch(db: &dyn salsa::Database, file: SourceFile) -> debian_watch::parse::Parse {
    let text = file.text(db);
    debian_watch::parse::Parse::parse(&text)
}

#[salsa::tracked]
pub fn parse_rules(
    db: &dyn salsa::Database,
    file: SourceFile,
) -> makefile_lossless::Parse<makefile_lossless::Makefile> {
    let text = file.text(db);
    makefile_lossless::Parse::parse_makefile(&text)
}

#[salsa::tracked]
pub fn parse_changelog(
    db: &dyn salsa::Database,
    file: SourceFile,
) -> debian_changelog::Parse<debian_changelog::ChangeLog> {
    let text = file.text(db);
    debian_changelog::ChangeLog::parse(&text)
}

#[salsa::tracked]
pub fn parse_deb822(
    db: &dyn salsa::Database,
    file: SourceFile,
) -> deb822_lossless::Parse<deb822_lossless::Deb822> {
    let text = file.text(db);
    deb822_lossless::Deb822::parse(&text)
}

// The actual database implementation
#[salsa::db]
#[derive(Clone, Default)]
pub struct Workspace {
    storage: salsa::Storage<Self>,
    files: HashMap<Uri, SourceFile>,
}

impl salsa::Database for Workspace {}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_file(&mut self, url: Uri, text: String) -> SourceFile {
        if let Some(&existing) = self.files.get(&url) {
            existing.set_text(self).to(text);
            existing
        } else {
            let sf = SourceFile::new(self, url.clone(), text);
            self.files.insert(url, sf);
            sf
        }
    }

    pub fn get_parsed_control(
        &self,
        file: SourceFile,
    ) -> debian_control::lossless::Parse<debian_control::lossless::Control> {
        parse_control(self, file)
    }

    pub fn source_text(&self, file: SourceFile) -> String {
        file.text(self).clone()
    }

    pub fn get_parsed_copyright(&self, file: SourceFile) -> debian_copyright::lossless::Parse {
        parse_copyright(self, file)
    }

    /// Find field casing issues in copyright files, optionally within a specific range
    pub fn find_copyright_field_casing_issues(
        &self,
        file: SourceFile,
        range: Option<TextRange>,
    ) -> Vec<FieldCasingIssue> {
        let mut issues = Vec::new();
        let copyright_parse = self.get_parsed_copyright(file);
        let copyright = copyright_parse.tree();

        // Check header fields
        if let Some(header) = copyright.header() {
            for entry in header.as_deb822().entries() {
                let entry_range = entry.text_range();

                // If a range is specified, check if this entry is within it
                if let Some(filter_range) = range {
                    if entry_range.start() >= filter_range.end()
                        || entry_range.end() <= filter_range.start()
                    {
                        continue; // Skip entries outside the range
                    }
                }

                if let Some(key) = entry.key() {
                    if let Some(standard_name) = crate::copyright::get_standard_field_name(&key) {
                        if key != standard_name {
                            if let Some(field_range) = entry.key_range() {
                                issues.push(FieldCasingIssue {
                                    field_name: key.to_string(),
                                    standard_name: standard_name.to_string(),
                                    field_range,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Check files paragraphs
        for files_para in copyright.iter_files() {
            for entry in files_para.as_deb822().entries() {
                let entry_range = entry.text_range();

                if let Some(filter_range) = range {
                    if entry_range.start() >= filter_range.end()
                        || entry_range.end() <= filter_range.start()
                    {
                        continue;
                    }
                }

                if let Some(key) = entry.key() {
                    if let Some(standard_name) = crate::copyright::get_standard_field_name(&key) {
                        if key != standard_name {
                            if let Some(field_range) = entry.key_range() {
                                issues.push(FieldCasingIssue {
                                    field_name: key.to_string(),
                                    standard_name: standard_name.to_string(),
                                    field_range,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Check license paragraphs
        for license_para in copyright.iter_licenses() {
            for entry in license_para.as_deb822().entries() {
                let entry_range = entry.text_range();

                if let Some(filter_range) = range {
                    if entry_range.start() >= filter_range.end()
                        || entry_range.end() <= filter_range.start()
                    {
                        continue;
                    }
                }

                if let Some(key) = entry.key() {
                    if let Some(standard_name) = crate::copyright::get_standard_field_name(&key) {
                        if key != standard_name {
                            if let Some(field_range) = entry.key_range() {
                                issues.push(FieldCasingIssue {
                                    field_name: key.to_string(),
                                    standard_name: standard_name.to_string(),
                                    field_range,
                                });
                            }
                        }
                    }
                }
            }
        }

        issues
    }

    pub fn get_copyright_diagnostics(&self, file: SourceFile) -> Vec<Diagnostic> {
        let source_text = self.source_text(file);
        let mut diagnostics = Vec::new();

        // Add field casing diagnostics
        for issue in self.find_copyright_field_casing_issues(file, None) {
            let lsp_range =
                crate::position::text_range_to_lsp_range(&source_text, issue.field_range);

            diagnostics.push(Diagnostic {
                range: lsp_range,
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(NumberOrString::String("field-casing".to_string())),
                source: Some("debian-lsp".to_string()),
                message: format!(
                    "Field name '{}' should be '{}'",
                    issue.field_name, issue.standard_name
                ),
                ..Default::default()
            });
        }

        diagnostics
    }

    pub fn get_parsed_rules(
        &self,
        file: SourceFile,
    ) -> makefile_lossless::Parse<makefile_lossless::Makefile> {
        parse_rules(self, file)
    }

    pub fn get_parsed_watch(&self, file: SourceFile) -> debian_watch::parse::Parse {
        parse_watch(self, file)
    }

    pub fn get_parsed_deb822(
        &self,
        file: SourceFile,
    ) -> deb822_lossless::Parse<deb822_lossless::Deb822> {
        parse_deb822(self, file)
    }

    pub fn get_parsed_changelog(
        &self,
        file: SourceFile,
    ) -> debian_changelog::Parse<debian_changelog::ChangeLog> {
        parse_changelog(self, file)
    }

    /// Find UNRELEASED entries in the given range that can be marked for upload
    pub fn find_unreleased_entries_in_range(
        &self,
        file: SourceFile,
        range: TextRange,
    ) -> Vec<UnreleasedUploadInfo> {
        let parsed = self.get_parsed_changelog(file);
        let changelog = parsed.tree();

        // Determine target distribution from previous entries
        let target_distribution = crate::changelog::get_target_distribution(&changelog);

        let mut results = Vec::new();

        // Use the new efficient entries_in_range API
        for entry in changelog.entries_in_range(range) {
            // Check if this entry has UNRELEASED
            if let Some(dists) = entry.distributions() {
                if !dists.is_empty() && dists[0] == "UNRELEASED" {
                    // Find the exact position of "UNRELEASED" in the entry's syntax tree
                    let entry_text = entry.syntax().text().to_string();
                    if let Some(offset) = entry_text.find(") UNRELEASED;") {
                        let unreleased_start = offset + 2; // +2 for ") "
                        let unreleased_end = unreleased_start + "UNRELEASED".len();

                        // Convert to absolute positions
                        let entry_range = entry.syntax().text_range();
                        let abs_start = entry_range.start()
                            + text_size::TextSize::from(unreleased_start as u32);
                        let abs_end =
                            entry_range.start() + text_size::TextSize::from(unreleased_end as u32);

                        results.push(UnreleasedUploadInfo {
                            unreleased_range: TextRange::new(abs_start, abs_end),
                            target_distribution: target_distribution.clone(),
                        });
                    }
                }
            }
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_copyright_with_correct_casing() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/copyright").unwrap();
        let content = r#"Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: test-package
Source: https://example.com/test

Files: *
Copyright: 2024 Test Author
License: MIT
"#;

        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_copyright(file);

        assert!(parsed.errors().is_empty());

        let copyright = parsed.tree();
        assert!(copyright.header().is_some());
        assert_eq!(copyright.iter_files().count(), 1);
    }

    #[test]
    fn test_parse_copyright_with_incorrect_casing() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/copyright").unwrap();
        let content = r#"format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
upstream-name: test-package
source: https://example.com/test

files: *
copyright: 2024 Test Author
license: MIT
"#;

        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_copyright(file);

        assert!(parsed.errors().is_empty());

        let copyright = parsed.tree();
        assert!(copyright.header().is_some());
        assert_eq!(copyright.iter_files().count(), 1);
    }

    #[test]
    fn test_copyright_field_casing_detection() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/copyright").unwrap();
        let content = r#"format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
upstream-name: test-package

files: *
copyright: 2024 Test Author
license: MIT
"#;

        let file = workspace.update_file(url, content.to_string());
        let issues = workspace.find_copyright_field_casing_issues(file, None);

        // Should detect incorrect casing for format, upstream-name, files, copyright, license
        assert!(issues.len() >= 3, "Expected at least 3 field casing issues");

        // Check that we detect specific fields
        let field_names: Vec<_> = issues.iter().map(|i| i.field_name.as_str()).collect();
        assert!(field_names.contains(&"format"));
        assert!(field_names.contains(&"files"));
        assert!(field_names.contains(&"license"));
    }

    #[test]
    fn test_copyright_diagnostics() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/copyright").unwrap();
        let content = r#"format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
upstream-name: test

files: *
copyright: 2024 Test
license: MIT
"#;

        let file = workspace.update_file(url, content.to_string());
        let diagnostics = workspace.get_copyright_diagnostics(file);

        assert!(
            !diagnostics.is_empty(),
            "Should have diagnostics for field casing"
        );

        // All diagnostics should be warnings for field casing
        for diag in &diagnostics {
            assert_eq!(diag.severity, Some(DiagnosticSeverity::WARNING));
            assert_eq!(
                diag.code,
                Some(NumberOrString::String("field-casing".to_string()))
            );
        }
    }

    #[test]
    fn test_parse_watch_linebased_v4() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/watch").unwrap();
        let content = "version=4\nhttps://example.com/files .*/foo-(\\d[\\d.]*)/.tar\\.gz\n";

        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_watch(file);

        assert_eq!(parsed.version(), 4);
    }

    #[test]
    fn test_parse_watch_deb822_v5() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/watch").unwrap();
        let content = r#"Version: 5

Source: https://github.com/owner/repo/tags
Matching-Pattern: .*/v?(\d[\d.]*)\.tar\.gz
"#;

        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_watch(file);

        assert_eq!(parsed.version(), 5);
    }

    #[test]
    fn test_parse_watch_auto_detect_v1() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/watch").unwrap();
        let content = "https://example.com/files .*/foo-(\\d[\\d.]*).tar\\.gz\n";

        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_watch(file);

        // Should default to version 1
        assert_eq!(parsed.version(), 1);
    }

    #[test]
    fn test_parse_changelog_basic() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/changelog").unwrap();
        let content = r#"rust-foo (0.1.0-1) unstable; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;

        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_changelog(file);

        assert!(parsed.errors().is_empty());
    }

    #[test]
    fn test_parse_changelog_multiple_entries() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/changelog").unwrap();
        let content = r#"rust-foo (0.2.0-1) unstable; urgency=high

  * New upstream release.
  * Fix security vulnerability.

 -- John Doe <john@example.com>  Tue, 02 Jan 2024 12:00:00 +0000

rust-foo (0.1.0-1) unstable; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;

        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_changelog(file);

        assert!(parsed.errors().is_empty());
    }

    #[test]
    fn test_find_unreleased_entries_in_range() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/changelog").unwrap();
        let content = r#"rust-foo (0.2.0-1) UNRELEASED; urgency=medium

  * New changes.

 -- John Doe <john@example.com>  Tue, 02 Jan 2024 12:00:00 +0000

rust-foo (0.1.0-1) unstable; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;

        let file = workspace.update_file(url, content.to_string());

        // Search the entire file
        let full_range = TextRange::new(0.into(), (content.len() as u32).into());
        let unreleased_entries = workspace.find_unreleased_entries_in_range(file, full_range);

        assert_eq!(unreleased_entries.len(), 1);
        assert_eq!(unreleased_entries[0].target_distribution, "unstable");

        // Verify the range points to "UNRELEASED"
        let unreleased_text = &content[unreleased_entries[0].unreleased_range.start().into()
            ..unreleased_entries[0].unreleased_range.end().into()];
        assert_eq!(unreleased_text, "UNRELEASED");
    }

    #[test]
    fn test_find_unreleased_entries_multiple() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/changelog").unwrap();
        let content = r#"rust-foo (0.3.0-1) UNRELEASED; urgency=medium

  * More new changes.

 -- John Doe <john@example.com>  Wed, 03 Jan 2024 12:00:00 +0000

rust-foo (0.2.0-1) UNRELEASED; urgency=medium

  * New changes.

 -- John Doe <john@example.com>  Tue, 02 Jan 2024 12:00:00 +0000

rust-foo (0.1.0-1) experimental; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;

        let file = workspace.update_file(url, content.to_string());

        // Search the entire file
        let full_range = TextRange::new(0.into(), (content.len() as u32).into());
        let unreleased_entries = workspace.find_unreleased_entries_in_range(file, full_range);

        // Should find both UNRELEASED entries
        assert_eq!(unreleased_entries.len(), 2);
        // Target should be "experimental" from the first released entry
        assert_eq!(unreleased_entries[0].target_distribution, "experimental");
        assert_eq!(unreleased_entries[1].target_distribution, "experimental");
    }

    #[test]
    fn test_find_unreleased_entries_partial_range() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/changelog").unwrap();
        let content = r#"rust-foo (0.3.0-1) UNRELEASED; urgency=medium

  * More new changes.

 -- John Doe <john@example.com>  Wed, 03 Jan 2024 12:00:00 +0000

rust-foo (0.2.0-1) UNRELEASED; urgency=medium

  * New changes.

 -- John Doe <john@example.com>  Tue, 02 Jan 2024 12:00:00 +0000

rust-foo (0.1.0-1) unstable; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;

        let file = workspace.update_file(url, content.to_string());

        // Search only the first entry (first 100 characters should be enough)
        let partial_range = TextRange::new(0.into(), 100.into());
        let unreleased_entries = workspace.find_unreleased_entries_in_range(file, partial_range);

        // Should find only the first UNRELEASED entry
        assert_eq!(unreleased_entries.len(), 1);
    }

    #[test]
    fn test_find_unreleased_entries_no_matches() {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/changelog").unwrap();
        let content = r#"rust-foo (0.1.0-1) unstable; urgency=medium

  * Initial release.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
"#;

        let file = workspace.update_file(url, content.to_string());

        // Search the entire file
        let full_range = TextRange::new(0.into(), (content.len() as u32).into());
        let unreleased_entries = workspace.find_unreleased_entries_in_range(file, full_range);

        // Should find no UNRELEASED entries
        assert_eq!(unreleased_entries.len(), 0);
    }
}
