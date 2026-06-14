//! Document link support for debian/tests/control files.
//!
//! Turns each name in a `Tests:` field into a clickable link to the test
//! script on disk. Scripts live under `debian/tests/` by default, or under the
//! directory named in the paragraph's `Tests-Directory:` field. A name only
//! produces a link when the resolved script exists.

use tower_lsp_server::ls_types::{DocumentLink, Uri};

use crate::position::Source;

const DEFAULT_TESTS_DIRECTORY: &str = "debian/tests";

/// Get document links for test names in the `Tests:` fields of a
/// debian/tests/control file.
///
/// `uri` is the control file's URI; test scripts are resolved relative to the
/// source-tree root (the parent of `debian/`).
pub fn get_document_links(
    deb822: &deb822_lossless::Deb822,
    src: Source<'_>,
    uri: &Uri,
) -> Vec<DocumentLink> {
    let Some(root) = source_root(uri) else {
        return Vec::new();
    };

    let mut links = Vec::new();
    for para in deb822.paragraphs() {
        let Some(entry) = para
            .entries()
            .find(|e| e.key().is_some_and(|k| k.eq_ignore_ascii_case("Tests")))
        else {
            continue;
        };
        let Some(vr) = entry.value_range() else {
            continue;
        };
        let value_start: usize = vr.start().into();
        let value_end: usize = vr.end().into();
        let value = &src.text[value_start..value_end];

        let tests_dir = para
            .get("Tests-Directory")
            .map(|v| root.join(v.trim()))
            .unwrap_or_else(|| root.join(DEFAULT_TESTS_DIRECTORY));

        for (rel_start, name) in iter_tokens(value) {
            let test_path = tests_dir.join(name);
            if !test_path.is_file() {
                continue;
            }
            let Some(target) = Uri::from_file_path(&test_path) else {
                continue;
            };
            let start = (value_start + rel_start) as u32;
            let end = start + name.len() as u32;
            links.push(DocumentLink {
                range: src
                    .text_range_to_lsp_range(text_size::TextRange::new(start.into(), end.into())),
                target: Some(target),
                tooltip: Some(name.to_string()),
                data: None,
            });
        }
    }

    links
}

/// Yield each whitespace-separated token in `value` with its byte offset.
fn iter_tokens(value: &str) -> impl Iterator<Item = (usize, &str)> {
    value
        .split_inclusive([' ', '\n'])
        .scan(0usize, |offset, segment| {
            let seg_start = *offset;
            *offset += segment.len();
            let token = segment.trim_end_matches([' ', '\n']);
            Some((seg_start, token))
        })
        .filter(|(_, token)| !token.is_empty())
}

/// Resolve the source-tree root (the directory containing `debian/`) from a
/// control URI of the form `.../debian/tests/control`.
fn source_root(uri: &Uri) -> Option<std::path::PathBuf> {
    let path = uri.to_file_path()?;
    // `<root>/debian/tests/control` -> `<root>`.
    Some(path.parent()?.parent()?.parent()?.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;

    fn write_tests_control(dir: &std::path::Path, content: &str) -> Uri {
        let tests_dir = dir.join("debian").join("tests");
        std::fs::create_dir_all(&tests_dir).unwrap();
        let control = tests_dir.join("control");
        std::fs::write(&control, content).unwrap();
        Uri::from_file_path(&control).unwrap()
    }

    fn links(uri: &Uri, content: &str) -> Vec<DocumentLink> {
        let deb822 = deb822_lossless::Deb822::parse(content).to_result().unwrap();
        let idx = LineIndex::new(content);
        get_document_links(&deb822, Source::new(content, &idx), uri)
    }

    #[test]
    fn links_existing_test() {
        let dir = tempfile::tempdir().unwrap();
        let content = "Tests: smoke\nDepends: @\n";
        let uri = write_tests_control(dir.path(), content);
        std::fs::write(dir.path().join("debian/tests/smoke"), "#!/bin/sh\n").unwrap();

        let ls = links(&uri, content);
        assert_eq!(ls.len(), 1);
        assert_eq!(
            ls[0].target.as_ref().unwrap(),
            &Uri::from_file_path(dir.path().join("debian/tests/smoke")).unwrap()
        );
        assert_eq!(ls[0].tooltip.as_deref(), Some("smoke"));
        // "smoke" starts at column 7 on line 0.
        assert_eq!(ls[0].range.start.line, 0);
        assert_eq!(ls[0].range.start.character, 7);
        assert_eq!(ls[0].range.end.character, 12);
    }

    #[test]
    fn links_multiple_tests() {
        let dir = tempfile::tempdir().unwrap();
        let content = "Tests: smoke integration\n";
        let uri = write_tests_control(dir.path(), content);
        std::fs::write(dir.path().join("debian/tests/smoke"), "#!/bin/sh\n").unwrap();
        std::fs::write(dir.path().join("debian/tests/integration"), "#!/bin/sh\n").unwrap();

        let ls = links(&uri, content);
        assert_eq!(ls.len(), 2);
        assert_eq!(ls[0].tooltip.as_deref(), Some("smoke"));
        assert_eq!(ls[1].tooltip.as_deref(), Some("integration"));
        assert_eq!(ls[1].range.start.character, 13);
    }

    #[test]
    fn skips_missing_test() {
        let dir = tempfile::tempdir().unwrap();
        let content = "Tests: smoke ghost\n";
        let uri = write_tests_control(dir.path(), content);
        std::fs::write(dir.path().join("debian/tests/smoke"), "#!/bin/sh\n").unwrap();

        let ls = links(&uri, content);
        assert_eq!(ls.len(), 1);
        assert_eq!(ls[0].tooltip.as_deref(), Some("smoke"));
    }

    #[test]
    fn respects_tests_directory() {
        let dir = tempfile::tempdir().unwrap();
        let content = "Tests: smoke\nTests-Directory: t\n";
        let uri = write_tests_control(dir.path(), content);
        let custom = dir.path().join("t");
        std::fs::create_dir_all(&custom).unwrap();
        std::fs::write(custom.join("smoke"), "#!/bin/sh\n").unwrap();

        let ls = links(&uri, content);
        assert_eq!(ls.len(), 1);
        assert_eq!(
            ls[0].target.as_ref().unwrap(),
            &Uri::from_file_path(custom.join("smoke")).unwrap()
        );
    }

    #[test]
    fn links_tests_across_paragraphs() {
        let dir = tempfile::tempdir().unwrap();
        let content = "Tests: smoke\n\nTests: integration\n";
        let uri = write_tests_control(dir.path(), content);
        std::fs::write(dir.path().join("debian/tests/smoke"), "#!/bin/sh\n").unwrap();
        std::fs::write(dir.path().join("debian/tests/integration"), "#!/bin/sh\n").unwrap();

        let ls = links(&uri, content);
        assert_eq!(ls.len(), 2);
        assert_eq!(ls[0].tooltip.as_deref(), Some("smoke"));
        assert_eq!(ls[1].tooltip.as_deref(), Some("integration"));
    }

    #[test]
    fn no_links_without_tests_field() {
        let dir = tempfile::tempdir().unwrap();
        let content = "Test-Command: ./run\n";
        let uri = write_tests_control(dir.path(), content);

        let ls = links(&uri, content);
        assert!(ls.is_empty());
    }

    #[test]
    fn iter_tokens_tracks_offsets() {
        let tokens: Vec<_> = iter_tokens("smoke integration").collect();
        assert_eq!(tokens, vec![(0, "smoke"), (6, "integration")]);
    }

    #[test]
    fn iter_tokens_handles_extra_whitespace() {
        let tokens: Vec<_> = iter_tokens(" smoke  integration ").collect();
        assert_eq!(tokens, vec![(1, "smoke"), (8, "integration")]);
    }
}
