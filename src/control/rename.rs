//! Rename support for binary package names in debian/control.
//!
//! When a binary package name is renamed, this module generates a workspace edit that:
//! - Updates the `Package:` field value in debian/control
//! - Renames `debian/<old_name>.*` files to `debian/<new_name>.*`
//! - Updates references in `debian/tests/control` Depends lines

use debian_control::lossless::{Control, Parse};
use std::path::Path;
use tower_lsp_server::ls_types::*;

use crate::position::{text_range_to_lsp_range, try_position_to_offset};

/// File extensions that are named after binary packages in the debian/ directory.
const PACKAGE_FILE_EXTENSIONS: &[&str] = &[
    "install",
    "docs",
    "dirs",
    "examples",
    "manpages",
    "links",
    "lintian-overrides",
    "bug-control",
    "bug-presubj",
    "bug-script",
    "cron.d",
    "cron.daily",
    "cron.hourly",
    "cron.monthly",
    "cron.weekly",
    "default",
    "logrotate",
    "postinst",
    "postrm",
    "preinst",
    "prerm",
    "triggers",
    "shlibs",
    "symbols",
    "templates",
    "config",
    "init",
    "pam",
    "menu",
    "mime",
    "maintscript",
    "service",
    "tmpfile",
    "udev",
    "upstart",
    "bash-completion",
];

/// Information about a binary package name at a cursor position.
pub struct PackageNameAtPosition {
    /// The current package name.
    pub name: String,
    /// The LSP range of the package name value in the source text.
    pub range: Range,
}

/// Find the binary package name at the given cursor position in a control file.
///
/// Returns `Some` if the cursor is on a `Package:` field value in a binary paragraph.
pub fn find_package_name_at_position(
    parsed: &Parse<Control>,
    source_text: &str,
    position: &Position,
) -> Option<PackageNameAtPosition> {
    let control = parsed.tree();
    let offset = try_position_to_offset(source_text, *position)?;

    for binary in control.binaries() {
        let para = binary.as_deb822();
        let entry = para.get_entry("Package")?;
        let value_range = entry.value_range()?;

        if value_range.contains(offset) || value_range.end() == offset {
            let name = binary.name()?;
            let lsp_range = text_range_to_lsp_range(source_text, value_range);
            return Some(PackageNameAtPosition {
                name,
                range: lsp_range,
            });
        }
    }

    None
}

/// Collect file rename operations for a binary package rename.
///
/// Scans the `debian/` directory for files named `<old_name>.<ext>` and generates
/// `RenameFile` operations to rename them to `<new_name>.<ext>`.
pub fn collect_package_file_renames(
    debian_dir: &Path,
    old_name: &str,
    new_name: &str,
) -> Vec<ResourceOp> {
    let mut ops = Vec::new();

    let Ok(entries) = std::fs::read_dir(debian_dir) else {
        return ops;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        // Check if this file is named <old_name>.<ext> where ext is a known extension
        let Some(rest) = file_name.strip_prefix(old_name) else {
            continue;
        };
        let Some(ext) = rest.strip_prefix('.') else {
            continue;
        };

        if !PACKAGE_FILE_EXTENSIONS.contains(&ext) {
            continue;
        }

        let new_path = debian_dir.join(format!("{new_name}.{ext}"));
        let Some(old_uri) = Uri::from_file_path(&path) else {
            continue;
        };
        let Some(new_uri) = Uri::from_file_path(&new_path) else {
            continue;
        };

        ops.push(ResourceOp::Rename(RenameFile {
            old_uri,
            new_uri,
            options: None,
            annotation_id: None,
        }));
    }

    ops
}

/// Generate text edits for updating package name references in a deb822-format
/// file (like `debian/tests/control`).
///
/// Searches all paragraphs for `Depends:` fields that reference the old package name
/// and generates edits to replace with the new name.
pub fn collect_tests_control_edits(
    tests_control_text: &str,
    old_name: &str,
    new_name: &str,
) -> Vec<TextEdit> {
    let parsed = deb822_lossless::Deb822::parse(tests_control_text);
    let deb822 = parsed.tree();
    let mut edits = Vec::new();

    for para in deb822.paragraphs() {
        let Some(entry) = para.get_entry("Depends") else {
            continue;
        };
        let Some(value_range) = entry.value_range() else {
            continue;
        };

        let value_start: usize = value_range.start().into();
        let value_end: usize = value_range.end().into();
        let value_text = &tests_control_text[value_start..value_end];

        // Find occurrences of the old package name in the Depends value.
        // We need to match whole package names, not substrings.
        let mut search_offset = 0;
        while let Some(pos) = value_text[search_offset..].find(old_name) {
            let abs_pos = search_offset + pos;
            let match_end = abs_pos + old_name.len();

            // Check that this is a whole-word match within the dependency list
            let before_ok = abs_pos == 0
                || !value_text.as_bytes()[abs_pos - 1].is_ascii_alphanumeric()
                    && value_text.as_bytes()[abs_pos - 1] != b'-';
            let after_ok = match_end >= value_text.len()
                || !value_text.as_bytes()[match_end].is_ascii_alphanumeric()
                    && value_text.as_bytes()[match_end] != b'-';

            if before_ok && after_ok {
                let abs_start = value_start + abs_pos;
                let abs_end = value_start + match_end;

                let start_range =
                    rowan::TextRange::new((abs_start as u32).into(), (abs_end as u32).into());
                let lsp_range = text_range_to_lsp_range(tests_control_text, start_range);

                edits.push(TextEdit {
                    range: lsp_range,
                    new_text: new_name.to_string(),
                });
            }

            search_offset = match_end;
        }
    }

    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_package_name_at_position() {
        let text = "\
Source: foo
Maintainer: Test <test@example.com>

Package: foo
Architecture: any
Description: Main package

Package: foo-dev
Architecture: any
Description: Development files
";
        let parsed = Control::parse(text);

        // Position on "foo" in "Package: foo" (line 3, character 9 = start of "foo")
        let pos = Position {
            line: 3,
            character: 9,
        };
        let result = find_package_name_at_position(&parsed, text, &pos);
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.name, "foo");

        // Position on "foo-dev" in "Package: foo-dev" (line 7)
        let pos = Position {
            line: 7,
            character: 9,
        };
        let result = find_package_name_at_position(&parsed, text, &pos);
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.name, "foo-dev");

        // Position on "Source:" line - should not match
        let pos = Position {
            line: 0,
            character: 8,
        };
        let result = find_package_name_at_position(&parsed, text, &pos);
        assert!(result.is_none());

        // Position on "Architecture:" line - should not match
        let pos = Position {
            line: 4,
            character: 5,
        };
        let result = find_package_name_at_position(&parsed, text, &pos);
        assert!(result.is_none());
    }

    #[test]
    fn test_collect_tests_control_edits() {
        let text = "\
Tests: test-foo
Depends: foo, bar, baz

Tests: test-foo-dev
Depends: foo-dev, foo
";
        let edits = collect_tests_control_edits(text, "foo", "qux");

        // Should find "foo" in first Depends (but not "foo-dev") and "foo" in second Depends
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].new_text, "qux");
        assert_eq!(edits[1].new_text, "qux");
    }

    #[test]
    fn test_collect_tests_control_edits_no_match() {
        let text = "\
Tests: test-bar
Depends: bar, baz
";
        let edits = collect_tests_control_edits(text, "foo", "qux");
        assert!(edits.is_empty());
    }

    #[test]
    fn test_collect_tests_control_edits_whole_word() {
        let text = "\
Tests: test
Depends: libfoo-dev, foo-dev, foo
";
        // Renaming "foo" should match "foo" but not "libfoo-dev" or "foo-dev"
        let edits = collect_tests_control_edits(text, "foo", "bar");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "bar");
        // The edit should point to the last "foo" on line 1
        assert_eq!(edits[0].range.start.line, 1);
    }

    #[test]
    fn test_collect_package_file_renames_empty_dir() {
        // Test with a non-existent directory
        let ops = collect_package_file_renames(Path::new("/nonexistent"), "foo", "bar");
        assert!(ops.is_empty());
    }
}
