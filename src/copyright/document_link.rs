//! Document link support for `debian/copyright` (DEP-5) files.
//!
//! Combines the generic deb822 URL links (the `Format` header, URLs embedded in
//! prose fields) with links for the literal paths named in `Files:` paragraphs:
//! a non-wildcard pattern that resolves to an existing file under the source
//! tree becomes a clickable jump to that file, mirroring the changelog's file
//! mentions and the SCIP indexer's `Files:` links.

use tower_lsp_server::ls_types::{DocumentLink, Uri};

use crate::deb822::completion::FieldInfo;
use crate::file_links::{file_link_target, source_root};
use crate::position::Source;
use text_size::TextRange;

/// Get document links for a `debian/copyright` file: URL-bearing fields plus
/// the literal file paths in `Files:` paragraphs.
///
/// `uri` is the copyright file's URI; `Files:` paths are resolved relative to
/// the source-tree root (the parent of `debian/`) and only link when the target
/// exists on disk.
pub fn get_document_links(
    copyright: &debian_copyright::lossless::Copyright,
    fields: &[FieldInfo],
    src: Source<'_>,
    uri: &Uri,
) -> Vec<DocumentLink> {
    let mut links =
        crate::deb822::document_link::get_document_links(copyright.as_deb822(), fields, src);

    if let Some(root) = source_root(uri) {
        for fp in copyright.iter_files() {
            for (pattern, span) in fp.file_spans() {
                let Some(rel) = file_link_target(&root, &pattern) else {
                    continue;
                };
                let Some(target) = Uri::from_file_path(root.join(&rel)) else {
                    continue;
                };
                let range = src.text_range_to_lsp_range(TextRange::new(span.start(), span.end()));
                links.push(DocumentLink {
                    range,
                    target: Some(target),
                    tooltip: Some(rel),
                    data: None,
                });
            }
        }
    }

    links
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::copyright::fields::COPYRIGHT_FIELDS;
    use crate::position::LineIndex;

    fn write_tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (rel, content) in files {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, content).unwrap();
        }
        dir
    }

    fn links(text: &str, uri: &Uri) -> Vec<DocumentLink> {
        let copyright = text
            .parse::<debian_copyright::lossless::Copyright>()
            .unwrap();
        let idx = LineIndex::new(text);
        get_document_links(&copyright, COPYRIGHT_FIELDS, Source::new(text, &idx), uri)
    }

    const CP: &str = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/main.c debian/copyright vendor/*
Copyright: 2026 Jelmer Vernooĳ <jelmer@debian.org>
License: GPL-2+

License: GPL-2+
 text
";

    #[test]
    fn links_literal_files_paths() {
        let dir = write_tree(&[("debian/copyright", CP), ("src/main.c", "int main() {}\n")]);
        let cp_path = dir.path().join("debian/copyright");
        let uri = Uri::from_file_path(&cp_path).unwrap();

        let ls = links(CP, &uri);
        let file_links: Vec<_> = ls
            .iter()
            .filter(|l| l.tooltip.as_deref() != Some("Format"))
            .collect();
        let tooltips: Vec<_> = file_links
            .iter()
            .map(|l| l.tooltip.clone().unwrap())
            .collect();
        // The two literal paths that exist link; `vendor/*` is a glob and is
        // skipped.
        assert_eq!(tooltips, vec!["src/main.c", "debian/copyright"]);

        // The src/main.c link targets the real file.
        let main_uri = Uri::from_file_path(dir.path().join("src/main.c")).unwrap();
        assert_eq!(file_links[0].target.as_ref().unwrap(), &main_uri);

        // The link covers just the path token.
        let idx = LineIndex::new(CP);
        let src = Source::new(CP, &idx);
        let start = src
            .try_position_to_offset(file_links[0].range.start)
            .unwrap();
        let end = src.try_position_to_offset(file_links[0].range.end).unwrap();
        assert_eq!(&CP[usize::from(start)..usize::from(end)], "src/main.c");
    }

    #[test]
    fn keeps_url_links() {
        let dir = write_tree(&[("debian/copyright", CP)]);
        let uri = Uri::from_file_path(dir.path().join("debian/copyright")).unwrap();
        let ls = links(CP, &uri);
        assert!(
            ls.iter().any(|l| l.tooltip.as_deref() == Some("Format")),
            "the Format URL link should still be produced"
        );
    }

    #[test]
    fn skips_missing_files_paths() {
        let text = "\
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/

Files: src/absent.c
Copyright: 2026 Alice
License: GPL-2+

License: GPL-2+
 text
";
        let dir = write_tree(&[("debian/copyright", text)]);
        let uri = Uri::from_file_path(dir.path().join("debian/copyright")).unwrap();
        let ls = links(text, &uri);
        assert!(
            !ls.iter()
                .any(|l| l.tooltip.as_deref() == Some("src/absent.c")),
            "a path that does not exist should not be linked"
        );
    }

    #[test]
    fn no_file_links_without_file_uri() {
        // A non-file URI yields no file links, but still produces URL links.
        let uri: Uri = "untitled:Untitled-1".parse().unwrap();
        let ls = links(CP, &uri);
        assert!(ls
            .iter()
            .all(|l| l.tooltip.as_deref() != Some("src/main.c")));
        assert!(ls.iter().any(|l| l.tooltip.as_deref() == Some("Format")));
    }
}
