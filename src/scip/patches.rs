//! Index `debian/patches/series` and the per-patch DEP-3 headers.
//!
//! Each non-comment line in `debian/patches/series` becomes a reference to a
//! patch symbol; each `debian/patches/<name>` file becomes a document whose
//! DEP-3 header is mined for hover information (Subject, Forwarded, Origin)
//! and for cross-links to BTS bug symbols emitted by the changelog indexer.
//!
//! The output aims for parity with the editor (LSP) features for these files:
//! syntax-highlighting occurrences for the series file, the patch headers and
//! the unified-diff body, a symbol per DEP-3 header field carrying the spec
//! description as hover documentation, and a synopsis on the patch symbol
//! itself.

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
    // Definition occurrence: anchor at the first line of the header. The whole
    // patch file is its enclosing scope.
    occurrences.push(Occurrence {
        range: lines.range(0, 0),
        symbol: patch_sym.clone(),
        symbol_roles: SymbolRole::Definition as i32,
        enclosing_range: lines.range(0, text.len() as u32),
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
    // description as hover documentation and deb822 highlighting. This covers
    // the header portion; the diff body is highlighted separately below.
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

    // Touched upstream file paths from the diff body. Parse with patchkit;
    // skip silently if the body is malformed (truncated, binary diff, etc.) —
    // anchored cross-references in the header are still useful on their own.
    emit_touched_paths(text, diff_offset, source, version, &lines, &mut occurrences);

    // Syntax-highlight the diff body itself (file markers, hunk headers,
    // added/removed lines) from patchkit's lossless syntax tree.
    if diff_offset < text.len() {
        occurrences.extend(crate::scip::highlight::diff(
            &text[diff_offset..],
            diff_offset as u32,
            &lines,
        ));
    }

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

    // The DEP-3 header paragraph is the enclosing scope of each of its fields.
    let para_range = paragraph.text_range();
    let enclosing_range = lines.range(para_range.start().into(), para_range.end().into());

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
            enclosing_range: enclosing_range.clone(),
            ..Default::default()
        });
        symbols_info.push(SymbolInformation {
            symbol: sym,
            kind: scip::types::symbol_information::Kind::Field.into(),
            display_name: name.clone(),
            documentation: dep3_field_doc(&name).into_iter().collect(),
            ..Default::default()
        });

        // Author/From carry a `Name <email>` identity; link the email so it
        // resolves to the same person across the archive.
        if name.eq_ignore_ascii_case("Author") || name.eq_ignore_ascii_case("From") {
            if let Some(vr) = entry.value_range() {
                let (vstart, vend): (usize, usize) = (vr.start().into(), vr.end().into());
                let value = &header_text[vstart..vend];
                if let Ok((_, email)) = debian_control::parse_identity(value) {
                    if !email.is_empty() {
                        // `email` is a trimmed slice into `value`, so its byte
                        // offset is the pointer difference -- exact, even if the
                        // same text also appears in the name part.
                        let rel = email.as_ptr() as usize - value.as_ptr() as usize;
                        let estart = (vstart + rel) as u32;
                        let eend = estart + email.len() as u32;
                        occurrences.push(lines.identity_occurrence(email, estart, eend));
                    }
                }
            }
        }
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
        // And the upstream path is cross-referenced.
        assert!(
            patch_doc
                .occurrences
                .iter()
                .any(|o| o.symbol.contains("upstream") && o.symbol.contains("src/foo.c")),
            "expected upstream-path ref to src/foo.c, got {:?}",
            patch_doc.occurrences
        );

        // The diff body is syntax-highlighted (hunk header, inserted line).
        assert!(has_kind(
            &patch_doc.occurrences,
            SyntaxKind::IdentifierFunction
        ));
        assert!(has_kind(&patch_doc.occurrences, SyntaxKind::StringLiteral));

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

        // The Author email is linked to a cross-archive identity symbol.
        let want = symbols::identity("jane@example.org");
        let id = doc
            .occurrences
            .iter()
            .find(|o| o.symbol == want)
            .expect("Author email identity occurrence");
        // Range covers just the email, not the surrounding `Name <...>`.
        assert_eq!(id.range, vec![0, 14, 0, 30]);
    }

    #[test]
    fn patch_and_field_definitions_carry_enclosing_range() {
        let text = "Author: Jane <jane@example.org>\nDescription: Fix the thing\nForwarded: no\n\n--- a/foo\n+++ b/foo\n@@ -1 +1 @@\n-a\n+b\n";
        let doc = index_patch(
            text,
            "debian/patches/p.patch",
            "hello",
            Some("1-1"),
            "p.patch",
        )
        .expect("patch document");
        for o in doc
            .occurrences
            .iter()
            .filter(|o| (o.symbol_roles & SymbolRole::Definition as i32) != 0)
        {
            assert!(
                !o.enclosing_range.is_empty(),
                "expected an enclosing range on {}",
                o.symbol
            );
        }
        // The patch symbol's enclosing range starts at the top of the file.
        let patch_sym = symbols::patch("hello", Some("1-1"), "p.patch");
        let patch_def = doc
            .occurrences
            .iter()
            .find(|o| o.symbol == patch_sym)
            .expect("patch definition");
        assert_eq!(
            patch_def.enclosing_range[..2],
            [0, 0],
            "patch enclosing range should start at the top of the file"
        );
    }

    #[test]
    fn from_header_email_is_linked() {
        // `From` is the git-format-patch alias for `Author`; its email links too.
        let text =
            "From: Jane Doe <jane@example.org>\n\n--- a/foo\n+++ b/foo\n@@ -1 +1 @@\n-a\n+b\n";
        let doc = index_patch(text, "debian/patches/p.patch", "hello", None, "p.patch")
            .expect("patch document");
        let want = symbols::identity("jane@example.org");
        assert!(
            doc.occurrences.iter().any(|o| o.symbol == want),
            "From email should be linked to an identity symbol"
        );
    }

    #[test]
    fn header_fields_do_not_leak_into_diff_body() {
        // A `+` line in the diff that looks like a field must not be indexed.
        let text = "Author: Jane\n\n--- a/foo\n+++ b/foo\n@@ -1 +1 @@\n+Forwarded: nope\n";
        let doc = index_patch(text, "debian/patches/p.patch", "hello", None, "p.patch")
            .expect("patch document");
        assert!(doc.symbols.iter().any(|s| s.symbol.contains("Author")));
        assert!(
            !doc.symbols.iter().any(|s| s.symbol.contains("Forwarded")),
            "diff-body text must not be parsed as a header field"
        );
    }
}
