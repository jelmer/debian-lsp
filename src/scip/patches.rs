//! Index `debian/patches/series` and the per-patch DEP-3 headers.
//!
//! Each non-comment line in `debian/patches/series` becomes a reference to a
//! patch symbol; each `debian/patches/<name>` file becomes a document whose
//! DEP-3 header is mined for hover information (Subject, Forwarded, Origin)
//! and for cross-links to BTS bug symbols emitted by the changelog indexer.
//!
//! The output aims for parity with the editor (LSP) features for these files:
//! syntax-highlighting occurrences for the series file and the patch headers,
//! a symbol per DEP-3 header field carrying the spec description as hover
//! documentation, and a synopsis on the patch symbol itself.

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
        if let Some(doc) = index_patch(root, &patch_text, &relative, source, version, &patch_name) {
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
    let parsed = patchkit::edit::series::parse(text);
    parsed
        .tree()
        .patch_entries()
        .filter_map(|p| p.name())
        .collect::<Vec<_>>()
        .into_iter()
}

fn index_series(text: &str, relative_path: &str, source: &str, version: Option<&str>) -> Document {
    let lines = LineTable::new(text);
    let parsed = patchkit::edit::series::parse(text);
    let series = parsed.tree();
    let mut occurrences = Vec::new();

    // Reference occurrences: each patch entry points at its patch symbol, so
    // "go to definition" on a series entry jumps to the patch document.
    for entry in series.patch_entries() {
        let (Some(name), Some(token)) = (entry.name(), entry.name_token()) else {
            continue;
        };
        let r = token.text_range();
        occurrences.push(Occurrence {
            range: lines.range(r.start().into(), r.end().into()),
            symbol: symbols::patch(source, version, &name),
            ..Default::default()
        });
    }

    // Syntax highlighting (patch names, options, comments).
    occurrences.extend(crate::scip::highlight::series(&series, &lines));

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
    root: &Path,
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
        documentation: patch_documentation(&header),
        relationships,
        ..Default::default()
    });

    // DEP-3 header fields: one symbol+occurrence each, with the spec
    // description as hover documentation and deb822 highlighting. This is the
    // header portion only; the diff body is left to diff-lsp.
    index_header_fields(
        text,
        diff_offset,
        source,
        version,
        patch_name,
        &lines,
        &mut occurrences,
        &mut symbols_info,
    );

    // Touched upstream file paths and hunk headers from the diff body, linked
    // to the file being patched. Parse with patchkit; skip silently if the body
    // is malformed (truncated, binary diff, etc.) — header cross-references are
    // still useful on their own.
    emit_touched_paths(
        root,
        text,
        diff_offset,
        &lines,
        &mut occurrences,
        &mut symbols_info,
    );

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

/// Build the hover documentation for a patch symbol from its DEP-3 header.
///
/// Mirrors what the editor shows: a synopsis line plus the forwarded status
/// and origin when present. Returns an empty vector if nothing is available.
fn patch_documentation(header: &PatchHeader) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(synopsis) = header.description() {
        let synopsis = synopsis.trim();
        if !synopsis.is_empty() {
            lines.push(synopsis.to_owned());
        }
    }
    let mut meta = Vec::new();
    if let Some(forwarded) = header.forwarded() {
        meta.push(format!("Forwarded: {forwarded}"));
    }
    if let Some((category, origin)) = header.origin() {
        match category {
            Some(cat) => meta.push(format!("Origin: {cat}, {origin}")),
            None => meta.push(format!("Origin: {origin}")),
        }
    }
    if !meta.is_empty() {
        lines.push(meta.join("  \n"));
    }
    if lines.is_empty() {
        Vec::new()
    } else {
        vec![lines.join("\n\n")]
    }
}

/// Emit a symbol, definition occurrence, and highlighting for each field in a
/// DEP-3 patch header.
///
/// Known fields carry their DEP-3 spec description as hover documentation;
/// unknown (vendor or `X-`) fields are still highlighted and given a symbol so
/// the outline stays complete.
#[allow(clippy::too_many_arguments)]
fn index_header_fields(
    text: &str,
    diff_offset: usize,
    source: &str,
    version: Option<&str>,
    patch_name: &str,
    lines: &LineTable,
    occurrences: &mut Vec<Occurrence>,
    symbols_info: &mut Vec<SymbolInformation>,
) {
    use deb822_lossless::SyntaxKind as Dk;
    use rowan::ast::AstNode;

    let header_text = &text[..diff_offset.min(text.len())];
    let parsed = deb822_lossless::Deb822::parse(header_text);
    let header = parsed.tree();
    let Some(paragraph) = header.paragraphs().next() else {
        return;
    };

    for entry in paragraph.entries() {
        let Some(name) = entry.key() else {
            continue;
        };
        // Anchor the field symbol on the KEY token so "go to definition" and
        // highlighting land on the field name itself.
        let key_range = entry
            .syntax()
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|it| it.kind() == Dk::KEY)
            .map(|t| t.text_range());
        let Some(key_range) = key_range else {
            continue;
        };
        let (start, end): (u32, u32) = (key_range.start().into(), key_range.end().into());

        let sym = symbols::patch_field(source, version, patch_name, &name);
        occurrences.push(Occurrence {
            range: lines.range(start, end),
            symbol: sym.clone(),
            symbol_roles: SymbolRole::Definition as i32,
            ..Default::default()
        });
        symbols_info.push(SymbolInformation {
            symbol: sym,
            kind: scip::types::symbol_information::Kind::Field.into(),
            display_name: name.clone(),
            documentation: dep3_field_doc(&name).into_iter().collect(),
            ..Default::default()
        });
    }

    // Syntax highlighting for the header (field names, values, comments). The
    // header is a prefix of the document, so deb822 offsets map straight onto
    // it; the diff body is left to diff-lsp.
    occurrences.extend(crate::scip::highlight::deb822(&header, lines));
}

/// DEP-3 spec description for a header field, if it is a known field.
fn dep3_field_doc(name: &str) -> Option<String> {
    let canonical = crate::dep3::fields::get_standard_field_name(name)?;
    crate::dep3::fields::DEP3_FIELDS
        .iter()
        .find(|f| f.name == canonical)
        .map(|f| f.description.to_owned())
}

/// Make the file paths and hunk headers in a patch's diff body clickable.
///
/// Each `--- a/path` / `+++ b/path` line and each `@@ ... @@` hunk header gets a
/// [`patch_target`](symbols::patch_target) symbol linking into the file being
/// patched at the touched line. The target line is the hunk's old-side start: in
/// a clean quilt checkout the file on disk is the unapplied upstream. Paths are
/// only linked when the file is present, mirroring the changelog file-mention
/// links.
///
/// Spans come from patchkit's lossless CST byte ranges rather than re-scanning.
fn emit_touched_paths(
    root: &Path,
    text: &str,
    diff_offset: usize,
    lines: &LineTable,
    occurrences: &mut Vec<Occurrence>,
    symbols_info: &mut Vec<SymbolInformation>,
) {
    use rowan::ast::AstNode;

    if diff_offset >= text.len() {
        return;
    }
    let body = &text[diff_offset..];
    let parsed = patchkit::edit::lossless::parse(body);
    let patch = parsed.tree();

    // CST offsets are relative to `body`; shift them onto the whole document.
    let base = diff_offset as u32;
    let mut seen = std::collections::HashSet::new();

    for file in patch.patch_files() {
        // Prefer the new side's path, falling back to the old side.
        let target = file
            .new_path()
            .map(|p| strip_patch_prefix(&p))
            .filter(|p| !p.is_empty())
            .or_else(|| file.old_path().map(|p| strip_patch_prefix(&p)))
            .filter(|p| !p.is_empty());
        let Some(target) = target else { continue };
        if !root.join(&target).is_file() {
            continue;
        }
        // The line the patch first touches, used as the path link's target.
        let first_line = file
            .hunks()
            .next()
            .and_then(|h| h.header())
            .and_then(|hdr| hdr.old_range())
            .and_then(|r| r.start())
            .unwrap_or(1)
            .max(1) as usize;

        // Both `--- a/path` and `+++ b/path` resolve to the same target.
        for path_token in [
            file.old_file().and_then(|f| f.path()),
            file.new_file().and_then(|f| f.path()),
        ]
        .into_iter()
        .flatten()
        {
            let r = path_token.text_range();
            emit_file_link(
                path_token.text(),
                &target,
                first_line,
                base + u32::from(r.start()),
                base + u32::from(r.end()),
                lines,
                occurrences,
                symbols_info,
                &mut seen,
            );
        }

        for hunk in file.hunks() {
            let Some(header) = hunk.header() else {
                continue;
            };
            let line = header
                .old_range()
                .and_then(|r| r.start())
                .unwrap_or(1)
                .max(1) as usize;
            // Trim the header node's trailing newline so the span stops at EOL.
            let r = header.syntax().text_range();
            let start = base + u32::from(r.start());
            let mut end = base + u32::from(r.end());
            if text[..end as usize].ends_with('\n') {
                end -= 1;
            }
            let label = &text[start as usize..end as usize];
            emit_file_link(
                label,
                &target,
                line,
                start,
                end,
                lines,
                occurrences,
                symbols_info,
                &mut seen,
            );
        }
    }
}

/// Emit a navigable link occurrence (and, on first sight, its symbol info)
/// spanning a byte range, targeting `relative_path` at 1-based `line`. `label` is
/// the link text shown to a consumer (the path or the hunk header). `seen`
/// deduplicates the symbol info so the path link and a hunk link that share a
/// target line document the symbol once.
#[allow(clippy::too_many_arguments)]
fn emit_file_link(
    label: &str,
    relative_path: &str,
    line: usize,
    start: u32,
    end: u32,
    lines: &LineTable,
    occurrences: &mut Vec<Occurrence>,
    symbols_info: &mut Vec<SymbolInformation>,
    seen: &mut std::collections::HashSet<String>,
) {
    let sym = symbols::patch_target(relative_path, line);
    occurrences.push(Occurrence {
        range: lines.range(start, end),
        symbol: sym.clone(),
        syntax_kind: scip::types::SyntaxKind::StringLiteral.into(),
        ..Default::default()
    });
    if seen.insert(sym.clone()) {
        symbols_info.push(SymbolInformation {
            symbol: sym,
            kind: scip::types::symbol_information::Kind::File.into(),
            display_name: relative_path.to_owned(),
            documentation: vec![symbols::patch_target_doc(label, relative_path, line)],
            ..Default::default()
        });
    }
}

/// Strip the leading `a/` or `b/` (or other `-p1`) component from a diff path,
/// yielding the repo-relative path. Quilt applies patches with `-p1`.
fn strip_patch_prefix(name: &str) -> String {
    patchkit::strip_prefix(Path::new(name), 1)
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use scip::types::SyntaxKind;
    use tempfile::tempdir;

    fn has_kind(occs: &[Occurrence], kind: SyntaxKind) -> bool {
        let want = kind.into();
        occs.iter().any(|o| o.syntax_kind == want)
    }

    #[test]
    fn series_entries_skip_comments_and_blanks() {
        let entries: Vec<_> = iter_series_entries(
            "# a comment\nfix-one.patch\n\nfix-two.patch -p1\n  # indented comment\n",
        )
        .collect();
        assert_eq!(entries, vec!["fix-one.patch", "fix-two.patch"]);
    }

    #[test]
    fn series_highlights_names_options_and_comments() {
        let doc = index_series(
            "# a comment\nfix-one.patch -p1\n",
            "debian/patches/series",
            "hello",
            Some("1-1"),
        );
        // The patch name resolves to its patch symbol (go-to-definition).
        assert!(doc
            .occurrences
            .iter()
            .any(|o| o.symbol.contains("fix-one.patch")));
        // Names, options, and comments are highlighted.
        assert!(has_kind(&doc.occurrences, SyntaxKind::IdentifierConstant));
        assert!(has_kind(&doc.occurrences, SyntaxKind::IdentifierParameter));
        assert!(has_kind(&doc.occurrences, SyntaxKind::Comment));
    }

    #[test]
    fn indexes_a_real_patches_tree() {
        let dir = tempdir().unwrap();
        let patches = dir.path().join("debian").join("patches");
        std::fs::create_dir_all(&patches).unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("foo.c"), "line1\nold\nline3\n").unwrap();
        std::fs::write(patches.join("series"), "fix-segfault.patch\n").unwrap();
        std::fs::write(
            patches.join("fix-segfault.patch"),
            "From: Jane <jane@example.org>\nSubject: Fix segfault\nBug-Debian: https://bugs.debian.org/123456\nForwarded: not-needed\n\n--- a/src/foo.c\n+++ b/src/foo.c\n@@ -1,3 +1,3 @@\n line1\n-old\n+new\n line3\n",
        )
        .unwrap();

        let idx = index(dir.path(), "hello", Some("2.10-3"));
        assert!(idx.series_document.is_some());
        assert_eq!(idx.patch_documents.len(), 1);

        // Series doc references the patch.
        let series = idx.series_document.unwrap();
        assert!(series
            .occurrences
            .iter()
            .any(|o| o.symbol.contains("fix-segfault.patch")));

        // Patch doc defines itself and cross-references the bug.
        let patch_doc = &idx.patch_documents[0];
        assert!(patch_doc
            .occurrences
            .iter()
            .any(|o| o.symbol.starts_with("scip-debian-bts")));
        // The touched file path links to src/foo.c at the patched line.
        assert!(
            patch_doc.symbols.iter().any(|s| s.documentation
                == vec![symbols::patch_target_doc("a/src/foo.c", "src/foo.c", 1)]),
            "expected patch-target link to src/foo.c#L1, got {:?}",
            patch_doc.symbols
        );

        // The patch symbol references the bug it closes and carries a synopsis.
        let patch_sym = patch_doc
            .symbols
            .iter()
            .find(|s| s.symbol.contains("patches") && s.symbol.contains("fix-segfault.patch"))
            .expect("patch symbol info");
        assert_eq!(patch_sym.display_name, "fix-segfault.patch");
        assert_eq!(patch_sym.documentation.len(), 1);
        assert!(patch_sym.documentation[0].contains("Fix segfault"));
        assert!(patch_sym.documentation[0].contains("not-needed"));
        assert_eq!(patch_sym.relationships.len(), 1);
        assert_eq!(
            patch_sym.relationships[0].symbol,
            symbols::bts_bug("123456")
        );
        assert!(patch_sym.relationships[0].is_reference);
    }

    #[test]
    fn patch_header_fields_get_symbols_and_docs() {
        let text = "Author: Jane <jane@example.org>\nDescription: Fix the thing\nForwarded: no\n\n--- a/foo\n+++ b/foo\n@@ -1 +1 @@\n-a\n+b\n";
        let doc = index_patch(
            Path::new("/nonexistent"),
            text,
            "debian/patches/p.patch",
            "hello",
            Some("1-1"),
            "p.patch",
        )
        .expect("patch document");

        // One field symbol per header field.
        let author = doc
            .symbols
            .iter()
            .find(|s| s.symbol.contains("Author"))
            .expect("Author field symbol");
        assert_eq!(author.display_name, "Author");
        assert!(!author.documentation.is_empty());
        assert!(doc.symbols.iter().any(|s| s.symbol.contains("Description")));
        assert!(doc.symbols.iter().any(|s| s.symbol.contains("Forwarded")));

        // Field names are highlighted as attributes; values as strings.
        assert!(has_kind(&doc.occurrences, SyntaxKind::IdentifierAttribute));
        assert!(has_kind(&doc.occurrences, SyntaxKind::StringLiteral));
    }

    #[test]
    fn header_fields_do_not_leak_into_diff_body() {
        // A `+` line in the diff that looks like a field must not be indexed.
        let text = "Author: Jane\n\n--- a/foo\n+++ b/foo\n@@ -1 +1 @@\n+Forwarded: nope\n";
        let doc = index_patch(
            Path::new("/nonexistent"),
            text,
            "debian/patches/p.patch",
            "hello",
            None,
            "p.patch",
        )
        .expect("patch document");
        assert!(doc.symbols.iter().any(|s| s.symbol.contains("Author")));
        assert!(
            !doc.symbols.iter().any(|s| s.symbol.contains("Forwarded")),
            "diff-body text must not be parsed as a header field"
        );
    }

    /// The patch-target symbols carried by the document's occurrences (the
    /// clickable spans), in document order. A `--- a/`/`+++ b/` path line and each
    /// `@@` hunk header each produce one occurrence.
    fn link_occurrences(doc: &Document) -> Vec<String> {
        doc.occurrences
            .iter()
            .map(|o| o.symbol.clone())
            .filter(|s| {
                doc.symbols
                    .iter()
                    .any(|si| &si.symbol == s && si.documentation.iter().any(|d| d.contains("#L")))
            })
            .collect()
    }

    /// The documented patch-target links (`[label](path#L<n>)`), one per distinct
    /// (path, line) target.
    fn link_docs(doc: &Document) -> Vec<String> {
        let mut docs: Vec<String> = doc
            .symbols
            .iter()
            .flat_map(|s| s.documentation.clone())
            .filter(|d| d.contains("#L"))
            .collect();
        docs.sort();
        docs
    }

    fn index_one_patch(patch: &str, present: &[(&str, &str)]) -> Document {
        let dir = tempdir().unwrap();
        let patches = dir.path().join("debian").join("patches");
        std::fs::create_dir_all(&patches).unwrap();
        for (path, body) in present {
            let abs = dir.path().join(path);
            std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
            std::fs::write(abs, body).unwrap();
        }
        std::fs::write(patches.join("series"), "p.patch\n").unwrap();
        std::fs::write(patches.join("p.patch"), patch).unwrap();
        let idx = index(dir.path(), "hello", Some("1-1"));
        idx.patch_documents.into_iter().next().expect("patch doc")
    }

    #[test]
    fn single_hunk_links_path_and_header_to_orig_line() {
        let patch = "Author: Jane\n\n--- a/src/foo.c\n+++ b/src/foo.c\n@@ -10,3 +10,3 @@\n line10\n-old\n+new\n line12\n";
        let doc = index_one_patch(patch, &[("src/foo.c", "x\n")]);
        // Three clickable tokens: the `---` path, the `+++` path, the hunk header.
        // All resolve to src/foo.c at the first touched line, so they share one
        // documented target.
        assert_eq!(
            link_occurrences(&doc),
            vec![
                symbols::patch_target("src/foo.c", 10),
                symbols::patch_target("src/foo.c", 10),
                symbols::patch_target("src/foo.c", 10),
            ]
        );
        assert_eq!(
            link_docs(&doc),
            vec![symbols::patch_target_doc("a/src/foo.c", "src/foo.c", 10)]
        );
    }

    #[test]
    fn each_hunk_links_to_its_own_line() {
        let patch = "Author: Jane\n\n--- a/m.c\n+++ b/m.c\n@@ -1,2 +1,2 @@\n a\n-b\n+B\n@@ -20,2 +20,2 @@\n y\n-z\n+Z\n";
        let doc = index_one_patch(patch, &[("m.c", "x\n")]);
        // Path lines and the first hunk all target line 1; the second hunk
        // targets line 20.
        assert_eq!(
            link_occurrences(&doc),
            vec![
                symbols::patch_target("m.c", 1),
                symbols::patch_target("m.c", 1),
                symbols::patch_target("m.c", 1),
                symbols::patch_target("m.c", 20),
            ]
        );
        assert_eq!(
            link_docs(&doc),
            vec![
                symbols::patch_target_doc("@@ -20,2 +20,2 @@", "m.c", 20),
                symbols::patch_target_doc("a/m.c", "m.c", 1),
            ]
        );
    }

    #[test]
    fn multiple_files_link_independently() {
        let patch = "Author: Jane\n\n--- a/one.c\n+++ b/one.c\n@@ -1 +1 @@\n-a\n+b\n--- a/two.c\n+++ b/two.c\n@@ -5 +5 @@\n-c\n+d\n";
        let doc = index_one_patch(patch, &[("one.c", "x\n"), ("two.c", "x\n")]);
        assert_eq!(
            link_occurrences(&doc),
            vec![
                symbols::patch_target("one.c", 1),
                symbols::patch_target("one.c", 1),
                symbols::patch_target("one.c", 1),
                symbols::patch_target("two.c", 5),
                symbols::patch_target("two.c", 5),
                symbols::patch_target("two.c", 5),
            ]
        );
    }

    #[test]
    fn git_style_diff_links_path_and_header() {
        // The `diff --git` / `index` metadata between files must be skipped.
        let patch = "Description: x\n\ndiff --git a/g.c b/g.c\nindex 1111111..2222222 100644\n--- a/g.c\n+++ b/g.c\n@@ -7,2 +7,2 @@\n a\n-b\n+B\n";
        let doc = index_one_patch(patch, &[("g.c", "x\n")]);
        assert_eq!(
            link_occurrences(&doc),
            vec![
                symbols::patch_target("g.c", 7),
                symbols::patch_target("g.c", 7),
                symbols::patch_target("g.c", 7),
            ]
        );
        assert_eq!(
            link_docs(&doc),
            vec![symbols::patch_target_doc("a/g.c", "g.c", 7)]
        );
    }

    #[test]
    fn p1_strips_only_the_leading_component() {
        // `-p1` removes the `a/`/`b/` prefix but leaves the rest of the path,
        // so a nested file resolves to its repo-relative path.
        let patch =
            "Author: Jane\n\n--- a/src/sub/foo.c\n+++ b/src/sub/foo.c\n@@ -1 +1 @@\n-a\n+b\n";
        let doc = index_one_patch(patch, &[("src/sub/foo.c", "x\n")]);
        assert_eq!(
            link_docs(&doc),
            vec![symbols::patch_target_doc(
                "a/src/sub/foo.c",
                "src/sub/foo.c",
                1
            )]
        );
    }

    #[test]
    fn absent_file_is_not_linked() {
        // The patched file is not on disk, so there is nothing to resolve to.
        let patch = "Author: Jane\n\n--- a/gone.c\n+++ b/gone.c\n@@ -1 +1 @@\n-a\n+b\n";
        let doc = index_one_patch(patch, &[]);
        assert_eq!(link_occurrences(&doc), Vec::<String>::new());
    }

    #[test]
    fn embedded_at_at_in_body_is_not_mistaken_for_a_header() {
        // A context line containing `@@ ` must not be paired with the second hunk.
        let patch = "Author: Jane\n\n--- a/c.c\n+++ b/c.c\n@@ -1,2 +1,2 @@\n a @@ b\n-x\n+y\n@@ -30,2 +30,2 @@\n p\n-q\n+r\n";
        let doc = index_one_patch(patch, &[("c.c", "x\n")]);
        assert_eq!(
            link_docs(&doc),
            vec![
                symbols::patch_target_doc("@@ -30,2 +30,2 @@", "c.c", 30),
                symbols::patch_target_doc("a/c.c", "c.c", 1),
            ]
        );
    }

    #[test]
    fn malformed_body_does_not_panic() {
        // A truncated hunk body must not panic; header cross-refs are unaffected.
        let patch = "Author: Jane\n\n--- a/foo.c\n+++ b/foo.c\n@@ -1,5 +1,5 @@\n only one line\n";
        let doc = index_one_patch(patch, &[("foo.c", "x\n")]);
        assert!(doc.symbols.iter().any(|s| s.symbol.contains("Author")));
    }
}
