//! Index `debian/patches/series` and the per-patch DEP-3 headers.
//!
//! Each non-comment line in `debian/patches/series` becomes a reference to a
//! patch symbol; each `debian/patches/<name>` file becomes a document whose
//! DEP-3 header is mined for hover information (Subject, Forwarded, Origin)
//! and for cross-links to BTS bug symbols emitted by the changelog indexer.

use crate::scip::linetable::LineTable;
use crate::scip::symbols;
use dep3::lossless::PatchHeader;
use scip::types::{Document, Occurrence, SymbolInformation, SymbolRole};
use std::path::Path;

/// Indexed result for `debian/patches/`.
pub struct PatchesIndex {
    /// The `series` document (may be empty if there is no series file).
    pub series_document: Option<Document>,
    /// One document per patch file with a parseable header.
    pub patch_documents: Vec<Document>,
}

/// Walk `<root>/debian/patches/` and produce SCIP documents.
///
/// `root` is the source-tree root; `source` and `version` flow into emitted
/// symbols.
pub fn index(root: &Path, source: &str, version: Option<&str>) -> PatchesIndex {
    let patches_dir = root.join("debian").join("patches");
    let series_path = patches_dir.join("series");

    let Ok(series_text) = std::fs::read_to_string(&series_path) else {
        return PatchesIndex {
            series_document: None,
            patch_documents: Vec::new(),
        };
    };
    let series_document = Some(index_series(
        &series_text,
        "debian/patches/series",
        source,
        version,
    ));

    let mut patch_documents = Vec::new();
    for patch_name in iter_series_entries(&series_text) {
        let patch_path = patches_dir.join(&patch_name);
        let Ok(patch_text) = std::fs::read_to_string(&patch_path) else {
            continue;
        };
        let relative = format!("debian/patches/{}", patch_name);
        if let Some(doc) = index_patch(&patch_text, &relative, source, version, &patch_name) {
            patch_documents.push(doc);
        }
    }

    PatchesIndex {
        series_document,
        patch_documents,
    }
}

/// Iterate the non-comment, non-blank entries of a `series` file.
fn iter_series_entries(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines().filter_map(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return None;
        }
        // A series line may carry quilt options after whitespace; take just
        // the first whitespace-delimited token.
        let name = trimmed.split_whitespace().next()?;
        Some(name.to_owned())
    })
}

fn index_series(text: &str, relative_path: &str, source: &str, version: Option<&str>) -> Document {
    let lines = LineTable::new(text);
    let mut occurrences = Vec::new();
    let mut offset: u32 = 0;
    for line in text.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len() as u32;
        let trimmed_line = line.trim_end_matches(['\n', '\r']);
        let stripped = trimmed_line.trim_start();
        if stripped.is_empty() || stripped.starts_with('#') {
            continue;
        }
        // Locate the first token (the patch filename).
        let leading_ws = trimmed_line.len() - stripped.len();
        let token_len = stripped.find(char::is_whitespace).unwrap_or(stripped.len());
        let name = &stripped[..token_len];
        let start = line_start + leading_ws as u32;
        let end = start + name.len() as u32;
        let sym = symbols::patch(source, version, name);
        occurrences.push(Occurrence {
            range: lines.range(start, end),
            symbol: sym,
            syntax_kind: scip::types::SyntaxKind::StringLiteral.into(),
            ..Default::default()
        });
    }
    Document {
        language: "plain".to_owned(),
        relative_path: relative_path.to_owned(),
        text: text.to_owned(),
        occurrences,
        position_encoding: scip::types::PositionEncoding::UTF8CodeUnitOffsetFromLineStart.into(),
        ..Default::default()
    }
}

fn index_patch(
    text: &str,
    relative_path: &str,
    source: &str,
    version: Option<&str>,
    patch_name: &str,
) -> Option<Document> {
    let (header, diff_offset) = PatchHeader::parse_relaxed(text).ok()?;
    let lines = LineTable::new(text);
    let mut occurrences = Vec::new();
    let mut symbols_info = Vec::new();

    let patch_sym = symbols::patch(source, version, patch_name);
    // Definition occurrence: anchor at the first line of the header.
    occurrences.push(Occurrence {
        range: lines.range(0, 0),
        symbol: patch_sym.clone(),
        symbol_roles: SymbolRole::Definition as i32,
        syntax_kind: scip::types::SyntaxKind::StringLiteral.into(),
        ..Default::default()
    });

    // The patch references each bug it closes, so "find references" on a BTS
    // bug surfaces the patches that fix it.
    let bug_ids: Vec<String> = header.debian_bug_ids().map(|id| id.to_string()).collect();
    let relationships = bug_ids
        .iter()
        .map(|id| symbols::rel_reference(symbols::bts_bug(id)))
        .collect();
    symbols_info.push(SymbolInformation {
        symbol: patch_sym,
        kind: scip::types::symbol_information::Kind::File.into(),
        display_name: patch_name.to_owned(),
        relationships,
        ..Default::default()
    });

    // Touched upstream file paths from the diff body. Parse with patchkit;
    // skip silently if the body is malformed (truncated, binary diff, etc.) —
    // anchored cross-references in the header are still useful on their own.
    emit_touched_paths(text, diff_offset, source, version, &lines, &mut occurrences);

    // BTS cross-reference occurrences via the Bug-Debian field.
    for id_text in &bug_ids {
        // Anchor the cross-reference at the first occurrence of the bug ID
        // in the file. dep3 doesn't expose token ranges, so a substring scan
        // is the pragmatic choice; the ID is specific enough to be unambiguous.
        let Some(pos) = text.find(id_text) else {
            continue;
        };
        let start = pos as u32;
        let end = start + id_text.len() as u32;
        occurrences.push(Occurrence {
            range: lines.range(start, end),
            symbol: symbols::bts_bug(id_text),
            syntax_kind: scip::types::SyntaxKind::NumericLiteral.into(),
            ..Default::default()
        });
    }

    Some(Document {
        language: "diff".to_owned(),
        relative_path: relative_path.to_owned(),
        text: text.to_owned(),
        occurrences,
        symbols: symbols_info,
        position_encoding: scip::types::PositionEncoding::UTF8CodeUnitOffsetFromLineStart.into(),
        ..Default::default()
    })
}

/// Emit one occurrence per file path referenced by the diff body of a patch.
///
/// Each `--- a/path` or `+++ b/path` line that patchkit recognises produces
/// a reference to the upstream-path symbol for that file. The leading `a/`
/// or `b/` prefix that quilt-style patches conventionally use is stripped.
fn emit_touched_paths(
    text: &str,
    diff_offset: usize,
    source: &str,
    version: Option<&str>,
    lines: &LineTable,
    occurrences: &mut Vec<Occurrence>,
) {
    if diff_offset >= text.len() {
        return;
    }
    let body = &text.as_bytes()[diff_offset..];
    let body_lines: Vec<Vec<u8>> = body
        .split_inclusive(|&b| b == b'\n')
        .map(|l| l.to_vec())
        .collect();
    for patch in patchkit::unified::parse_patches(body_lines.into_iter()) {
        let Ok(patchkit::unified::PlainOrBinaryPatch::Plain(up)) = patch else {
            continue;
        };
        for raw in [up.orig_name.as_slice(), up.mod_name.as_slice()] {
            let Ok(name) = std::str::from_utf8(raw) else {
                continue;
            };
            let stripped = strip_quilt_prefix(name);
            if stripped.is_empty() {
                continue;
            }
            // Anchor the occurrence at the first appearance of the raw path
            // string in the document. We search for `name` (with the prefix)
            // because that's what's literally on the `---`/`+++` line.
            let Some(pos) = text[diff_offset..].find(name) else {
                continue;
            };
            let abs = (diff_offset + pos) as u32;
            occurrences.push(Occurrence {
                range: lines.range(abs, abs + name.len() as u32),
                symbol: symbols::upstream_path(source, version, stripped),
                syntax_kind: scip::types::SyntaxKind::StringLiteral.into(),
                ..Default::default()
            });
        }
    }
}

fn strip_quilt_prefix(name: &str) -> &str {
    name.strip_prefix("a/")
        .or_else(|| name.strip_prefix("b/"))
        .unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn series_entries_skip_comments_and_blanks() {
        let entries: Vec<_> = iter_series_entries(
            "# a comment\nfix-one.patch\n\nfix-two.patch -p1\n  # indented comment\n",
        )
        .collect();
        assert_eq!(entries, vec!["fix-one.patch", "fix-two.patch"]);
    }

    #[test]
    fn indexes_a_real_patches_tree() {
        let dir = tempdir().unwrap();
        let patches = dir.path().join("debian").join("patches");
        std::fs::create_dir_all(&patches).unwrap();
        std::fs::write(patches.join("series"), "fix-segfault.patch\n").unwrap();
        std::fs::write(
            patches.join("fix-segfault.patch"),
            "From: Jane <jane@example.org>\nSubject: Fix segfault\nBug-Debian: https://bugs.debian.org/123456\n\n--- a/src/foo.c\n+++ b/src/foo.c\n@@ -1,3 +1,3 @@\n line1\n-old\n+new\n line3\n",
        )
        .unwrap();

        let idx = index(dir.path(), "hello", Some("2.10-3"));
        assert!(idx.series_document.is_some());
        assert_eq!(idx.patch_documents.len(), 1);

        // Series doc references the patch.
        let series = idx.series_document.unwrap();
        assert_eq!(series.occurrences.len(), 1);
        assert!(series.occurrences[0].symbol.contains("fix-segfault.patch"));

        // Patch doc defines itself and cross-references the bug.
        let patch_doc = &idx.patch_documents[0];
        assert!(patch_doc
            .occurrences
            .iter()
            .any(|o| o.symbol.starts_with("scip-debian-bts")));
        // And the upstream path is cross-referenced.
        assert!(
            patch_doc
                .occurrences
                .iter()
                .any(|o| o.symbol.contains("upstream") && o.symbol.contains("src/foo.c")),
            "expected upstream-path ref to src/foo.c, got {:?}",
            patch_doc.occurrences
        );

        // The patch symbol references the bug it closes.
        let patch_sym = patch_doc
            .symbols
            .iter()
            .find(|s| s.symbol.contains("patches") && s.symbol.contains("fix-segfault.patch"))
            .expect("patch symbol info");
        assert_eq!(patch_sym.relationships.len(), 1);
        assert_eq!(
            patch_sym.relationships[0].symbol,
            symbols::bts_bug("123456")
        );
        assert!(patch_sym.relationships[0].is_reference);
    }
}
