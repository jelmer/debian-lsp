//! Document link support for debian/changelog files.
//!
//! Turns mentions in changelog detail lines into clickable links:
//!
//! - packaging files (e.g. `d/control`, `debian/patches/03_fix.patch`) jump
//!   to the referenced file when it exists on disk;
//! - bug references (`Closes: #NNN`, `LP: #NNN`) open the bug on its tracker;
//! - GHSA identifiers open the GitHub Advisory Database.

use debian_changelog::bugs::iter_bug_refs;
use debian_changelog::SyntaxKind;
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{DocumentLink, Range, Uri};

use crate::changelog::file_refs::{iter_file_refs, iter_patch_word_refs};
use crate::position::Source;

/// Get document links for file and bug mentions in a changelog file.
///
/// `uri` is the changelog's URI; file paths are resolved relative to the
/// source-tree root (the parent of `debian/`). File mentions only produce a
/// link when they resolve to an existing file; bug references always link to
/// their tracker's web page.
pub fn get_document_links(
    parse: &debian_changelog::Parse<debian_changelog::ChangeLog>,
    src: Source<'_>,
    uri: &Uri,
) -> Vec<DocumentLink> {
    let Some(changelog) = debian_changelog::ChangeLog::cast(parse.syntax_node()) else {
        return Vec::new();
    };
    let root = crate::file_links::source_root(uri);

    let mut links = Vec::new();
    for token in changelog
        .syntax()
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
    {
        if token.kind() != SyntaxKind::DETAIL {
            continue;
        }
        let detail_start: usize = token.text_range().start().into();
        let detail_text = token.text();

        // File mentions resolve against the source tree on disk. Explicit
        // `d/...` paths and the prose `patch <name>` form are both gated on
        // the target existing, which also disambiguates the loose patch-word
        // heuristic.
        if let Some(root) = root.as_deref() {
            let mentions = iter_file_refs(detail_text)
                .into_iter()
                .chain(iter_patch_word_refs(detail_text));
            for file_ref in mentions {
                let target_path = root.join(&file_ref.path);
                if !target_path.is_file() {
                    continue;
                }
                let Some(target) = Uri::from_file_path(&target_path) else {
                    continue;
                };
                links.push(DocumentLink {
                    range: span_range(src, detail_start, file_ref.start, file_ref.end),
                    target: Some(target),
                    tooltip: Some(file_ref.path),
                    data: None,
                });
            }
        }

        // Bug references link to the tracker's web page.
        for bug_ref in iter_bug_refs(detail_text) {
            let Ok(target) = bug_ref.bug.url().parse::<Uri>() else {
                continue;
            };
            links.push(DocumentLink {
                range: span_range(src, detail_start, bug_ref.start, bug_ref.end),
                target: Some(target),
                tooltip: None,
                data: None,
            });
        }

        // GHSA identifiers link to the GitHub Advisory Database.
        for ghsa_ref in crate::ghsa::find_ghsas(detail_text) {
            let Ok(target) = crate::ghsa::advisory_url(&ghsa_ref.id).parse::<Uri>() else {
                continue;
            };
            links.push(DocumentLink {
                range: span_range(src, detail_start, ghsa_ref.start, ghsa_ref.end),
                target: Some(target),
                tooltip: None,
                data: None,
            });
        }
    }

    links
}

/// Build an LSP range for a `[rel_start, rel_end)` span within a detail token
/// that begins at byte `detail_start` in the document.
fn span_range(src: Source<'_>, detail_start: usize, rel_start: usize, rel_end: usize) -> Range {
    let start = (detail_start + rel_start) as u32;
    let end = (detail_start + rel_end) as u32;
    src.text_range_to_lsp_range(text_size::TextRange::new(start.into(), end.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;

    fn parse(text: &str) -> debian_changelog::Parse<debian_changelog::ChangeLog> {
        debian_changelog::ChangeLog::parse(text)
    }

    fn write_tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (rel, content) in files {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, content).unwrap();
        }
        dir
    }

    const CL: &str = "\
hello (2.10-3) unstable; urgency=medium

  * d/control: Add python3-merge3 as Build Dependency.
  * d/patches/03_fix.patch: Remove patch.
  * d/missing.conf: nothing here.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Tue, 27 May 2026 12:00:00 +0000
";

    #[test]
    fn links_existing_files_only() {
        let dir = write_tree(&[
            ("debian/changelog", CL),
            ("debian/control", "Source: hello\n"),
            ("debian/patches/03_fix.patch", "--- a/x\n+++ b/x\n"),
        ]);
        let cl_path = dir.path().join("debian/changelog");
        let uri = Uri::from_file_path(&cl_path).unwrap();
        let idx = LineIndex::new(CL);
        let src = Source::new(CL, &idx);

        let links = get_document_links(&parse(CL), src, &uri);
        let tooltips: Vec<_> = links.iter().map(|l| l.tooltip.clone().unwrap()).collect();
        assert_eq!(
            tooltips,
            vec!["debian/control", "debian/patches/03_fix.patch"]
        );

        // The control link targets the real file.
        let control_uri = Uri::from_file_path(dir.path().join("debian/control")).unwrap();
        assert_eq!(links[0].target.as_ref().unwrap(), &control_uri);
    }

    #[test]
    fn links_patch_word_when_patch_exists() {
        let text = "\
hello (2.10-3) unstable; urgency=medium

  * Drop obsolete patch relax-pyo3.patch.
  * Add patch nonexistent.patch.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Tue, 27 May 2026 12:00:00 +0000
";
        let dir = write_tree(&[
            ("debian/changelog", text),
            ("debian/patches/relax-pyo3.patch", "--- a/x\n+++ b/x\n"),
        ]);
        let uri = Uri::from_file_path(dir.path().join("debian/changelog")).unwrap();
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);

        let links = get_document_links(&parse(text), src, &uri);
        // Only the existing patch links; `nonexistent.patch` is filtered out.
        let tooltips: Vec<_> = links.iter().map(|l| l.tooltip.clone().unwrap()).collect();
        assert_eq!(tooltips, vec!["debian/patches/relax-pyo3.patch"]);

        // The link covers just the patch name, not the word "patch".
        let start = src.try_position_to_offset(links[0].range.start).unwrap();
        let end = src.try_position_to_offset(links[0].range.end).unwrap();
        assert_eq!(
            &text[usize::from(start)..usize::from(end)],
            "relax-pyo3.patch"
        );
    }

    #[test]
    fn no_file_links_without_file_uri() {
        // A non-file URI yields no file links rather than erroring; CL has no
        // bug references either, so the result is empty.
        let uri: Uri = "untitled:Untitled-1".parse().unwrap();
        let idx = LineIndex::new(CL);
        let src = Source::new(CL, &idx);
        let links = get_document_links(&parse(CL), src, &uri);
        assert!(links.is_empty());
    }

    #[test]
    fn links_bug_references() {
        let text = "\
hello (2.10-3) unstable; urgency=medium

  * Fix segfault. (Closes: #999888)
  * Sync with upstream. (LP: #1234567)

 -- Jelmer Vernooĳ <jelmer@debian.org>  Tue, 27 May 2026 12:00:00 +0000
";
        // A non-file URI still yields bug links.
        let uri: Uri = "untitled:Untitled-1".parse().unwrap();
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let links = get_document_links(&parse(text), src, &uri);

        let targets: Vec<_> = links
            .iter()
            .map(|l| l.target.as_ref().unwrap().as_str().to_owned())
            .collect();
        assert_eq!(
            targets,
            vec![
                "https://bugs.debian.org/999888",
                "https://bugs.launchpad.net/bugs/1234567",
            ]
        );

        // The link covers just the digits.
        let bug_link = &links[0];
        let start = src.try_position_to_offset(bug_link.range.start).unwrap();
        let end = src.try_position_to_offset(bug_link.range.end).unwrap();
        assert_eq!(&text[usize::from(start)..usize::from(end)], "999888");
    }

    #[test]
    fn links_ghsa_identifiers() {
        let text = "\
hello (2.10-3) unstable; urgency=medium

  * Fix advisory GHSA-jfh8-c2jp-5v3q.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Tue, 27 May 2026 12:00:00 +0000
";
        let uri: Uri = "untitled:Untitled-1".parse().unwrap();
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let links = get_document_links(&parse(text), src, &uri);

        let targets: Vec<_> = links
            .iter()
            .map(|l| l.target.as_ref().unwrap().as_str().to_owned())
            .collect();
        assert_eq!(
            targets,
            vec!["https://github.com/advisories/ghsa-jfh8-c2jp-5v3q"]
        );

        // The link covers just the identifier.
        let start = src.try_position_to_offset(links[0].range.start).unwrap();
        let end = src.try_position_to_offset(links[0].range.end).unwrap();
        assert_eq!(
            &text[usize::from(start)..usize::from(end)],
            "GHSA-jfh8-c2jp-5v3q"
        );
    }
}
