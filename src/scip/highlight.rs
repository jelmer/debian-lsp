//! Syntax-highlighting occurrences for SCIP documents.
//!
//! SCIP consumers (e.g. Sourcegraph) render syntax highlighting from
//! symbol-less [`Occurrence`]s whose `syntax_kind` field is set. This module
//! produces those occurrences by walking a file's syntax tree, separately from
//! the symbol/reference occurrences the per-file indexers emit.

use crate::scip::linetable::LineTable;
use scip::types::{Occurrence, SyntaxKind};

/// Build a highlight occurrence covering `[start, end)` with the given kind.
pub fn occurrence(lines: &LineTable, start: u32, end: u32, kind: SyntaxKind) -> Occurrence {
    Occurrence {
        range: lines.range(start, end),
        syntax_kind: kind.into(),
        ..Default::default()
    }
}

/// Emit highlight occurrences for a deb822 document.
///
/// Comments become [`SyntaxKind::Comment`], field names
/// [`SyntaxKind::IdentifierAttribute`], and field values
/// [`SyntaxKind::StringLiteral`].
pub fn deb822(deb822: &deb822_lossless::Deb822, lines: &LineTable) -> Vec<Occurrence> {
    use deb822_lossless::SyntaxKind as Dk;
    use rowan::ast::AstNode;

    let mut out = Vec::new();
    for element in deb822.syntax().descendants_with_tokens() {
        let rowan::NodeOrToken::Token(token) = element else {
            continue;
        };
        let kind = match token.kind() {
            Dk::COMMENT => SyntaxKind::Comment,
            Dk::KEY => SyntaxKind::IdentifierAttribute,
            Dk::VALUE => SyntaxKind::StringLiteral,
            _ => continue,
        };
        let r = token.text_range();
        let start: u32 = r.start().into();
        let end: u32 = r.end().into();
        if end > start {
            out.push(occurrence(lines, start, end, kind));
        }
    }
    out
}

/// Emit highlight occurrences for a `debian/rules` Makefile.
///
/// Comments, target names, variable names/references, operators and the
/// punctuation of `$(...)` references are classified for highlighting.
pub fn makefile(makefile: &makefile_lossless::Makefile, lines: &LineTable) -> Vec<Occurrence> {
    use makefile_lossless::SyntaxKind as Mk;
    use rowan::ast::AstNode;

    let mut out = Vec::new();
    for element in makefile.syntax().descendants_with_tokens() {
        let rowan::NodeOrToken::Token(token) = element else {
            continue;
        };
        let parent = token.parent().map(|p| p.kind());
        let kind = match token.kind() {
            Mk::COMMENT => SyntaxKind::Comment,
            Mk::IDENTIFIER => match parent {
                Some(Mk::TARGETS) => SyntaxKind::IdentifierFunction,
                Some(Mk::VARIABLE) | Some(Mk::EXPR) => SyntaxKind::IdentifierMutableGlobal,
                _ => continue,
            },
            Mk::OPERATOR => SyntaxKind::IdentifierOperator,
            Mk::DOLLAR => SyntaxKind::PunctuationDelimiter,
            Mk::LPAREN | Mk::RPAREN | Mk::LBRACE | Mk::RBRACE => SyntaxKind::PunctuationBracket,
            _ => continue,
        };
        push_token(&mut out, lines, &token, kind);
    }
    out
}

/// Emit highlight occurrences for a `debian/patches/series` file.
///
/// Patch names are classified as constants, quilt options as parameters, and
/// `#` comment lines as comments, mirroring the editor's semantic tokens for
/// these files.
pub fn series(
    series: &patchkit::edit::series::lossless::SeriesFile,
    lines: &LineTable,
) -> Vec<Occurrence> {
    use patchkit::edit::series::lex::SyntaxKind as Sk;
    use rowan::ast::AstNode;

    let mut out = Vec::new();
    for element in series.syntax().descendants_with_tokens() {
        let rowan::NodeOrToken::Token(token) = element else {
            continue;
        };
        let kind = match token.kind() {
            Sk::PATCH_NAME => SyntaxKind::IdentifierConstant,
            Sk::OPTION => SyntaxKind::IdentifierParameter,
            Sk::HASH | Sk::TEXT => SyntaxKind::Comment,
            _ => continue,
        };
        push_token(&mut out, lines, &token, kind);
    }
    out
}

/// Emit highlight occurrences for the unified-diff body of a patch file.
///
/// `body` is the slice of the document starting at `base` (the byte offset
/// where the diff body begins, i.e. just after the DEP-3 header). The body is
/// parsed into patchkit's lossless syntax tree and each line/header node is
/// highlighted by its node kind:
///
/// - file-header lines (`--- `/`+++ `) -> [`SyntaxKind::IdentifierNamespace`]
/// - hunk headers (`@@ ... @@`) -> [`SyntaxKind::IdentifierFunction`]
/// - added lines (`+`) -> [`SyntaxKind::StringLiteral`]
/// - removed/changed lines (`-`/`!`) -> [`SyntaxKind::Comment`]
/// - the `\ No newline at end of file` marker -> [`SyntaxKind::Comment`]
///
/// Context lines carry no highlight. Node ranges are relative to `body`, so
/// `base` is added to map them back onto the whole document.
pub fn diff(body: &str, base: u32, lines: &LineTable) -> Vec<Occurrence> {
    use patchkit::edit::lex::SyntaxKind as Pk;
    use rowan::ast::AstNode;

    let parsed = patchkit::edit::parse(body);
    let mut out = Vec::new();
    for node in parsed.tree().syntax().descendants() {
        let kind = match node.kind() {
            Pk::OLD_FILE | Pk::NEW_FILE | Pk::CONTEXT_OLD_FILE | Pk::CONTEXT_NEW_FILE => {
                SyntaxKind::IdentifierNamespace
            }
            Pk::HUNK_HEADER | Pk::CONTEXT_HUNK_HEADER => SyntaxKind::IdentifierFunction,
            Pk::ADD_LINE => SyntaxKind::StringLiteral,
            Pk::DELETE_LINE | Pk::CONTEXT_CHANGE_LINE => SyntaxKind::Comment,
            Pk::NO_NEWLINE_LINE => SyntaxKind::Comment,
            _ => continue,
        };
        let r = node.text_range();
        let (rstart, rend) = (usize::from(r.start()), usize::from(r.end()));
        // Trim a trailing newline so the highlight stops at the line break.
        let span = body[rstart..rend].trim_end_matches('\n');
        let start = base + rstart as u32;
        let end = start + span.len() as u32;
        if end > start {
            out.push(occurrence(lines, start, end, kind));
        }
    }
    out
}

/// Emit highlight occurrences for a `debian/changelog` file.
pub fn changelog(cl: &debian_changelog::ChangeLog, lines: &LineTable) -> Vec<Occurrence> {
    use debian_changelog::SyntaxKind as Ck;
    use rowan::ast::AstNode;

    let mut out = Vec::new();
    for element in cl.syntax().descendants_with_tokens() {
        let rowan::NodeOrToken::Token(token) = element else {
            continue;
        };
        let parent = token.parent().map(|p| p.kind());
        let kind = match token.kind() {
            Ck::COMMENT => SyntaxKind::Comment,
            Ck::VERSION => SyntaxKind::IdentifierConstant,
            Ck::IDENTIFIER => match parent {
                Some(Ck::ENTRY_HEADER) => SyntaxKind::IdentifierNamespace,
                Some(Ck::METADATA_KEY) => SyntaxKind::IdentifierAttribute,
                Some(Ck::DISTRIBUTIONS) => SyntaxKind::Identifier,
                Some(Ck::METADATA_VALUE) => SyntaxKind::StringLiteral,
                _ => continue,
            },
            Ck::TIMESTAMP => SyntaxKind::NumericLiteral,
            _ => match parent {
                Some(Ck::METADATA_VALUE) => SyntaxKind::StringLiteral,
                Some(Ck::TIMESTAMP) => SyntaxKind::NumericLiteral,
                Some(Ck::MAINTAINER) | Some(Ck::EMAIL) => SyntaxKind::StringLiteral,
                _ => continue,
            },
        };
        push_token(&mut out, lines, &token, kind);
    }
    out
}

/// Emit highlight occurrences for a `debian/watch` file.
///
/// Routes the deb822 (v5) format through [`deb822`] and the line-based (v1-4)
/// format through the makefile-style token walk.
pub fn watch(text: &str, lines: &LineTable) -> Vec<Occurrence> {
    let parsed = debian_watch::parse::Parse::parse(text);
    match parsed.to_watch_file() {
        debian_watch::parse::ParsedWatchFile::Deb822(wf) => deb822(wf.as_deb822(), lines),
        debian_watch::parse::ParsedWatchFile::LineBased(_) => watch_linebased(text, lines),
    }
}

/// Emit highlight occurrences for a line-based (v1-4) `debian/watch` file.
fn watch_linebased(text: &str, lines: &LineTable) -> Vec<Occurrence> {
    use debian_watch::SyntaxKind as Wk;

    let parsed = debian_watch::linebased::parse_watch_file(text);
    let wf = parsed.tree();
    let mut out = Vec::new();
    for element in wf.syntax().descendants_with_tokens() {
        let rowan::NodeOrToken::Token(token) = element else {
            continue;
        };
        let parent = token.parent().map(|p| p.kind());
        let kind = match token.kind() {
            Wk::COMMENT => SyntaxKind::Comment,
            Wk::KEY => SyntaxKind::IdentifierAttribute,
            Wk::VALUE => match parent {
                Some(Wk::VERSION) => SyntaxKind::NumericLiteral,
                Some(Wk::URL) => SyntaxKind::StringLiteral,
                Some(Wk::MATCHING_PATTERN) | Some(Wk::VERSION_POLICY) => SyntaxKind::RegexDelimiter,
                Some(Wk::OPTION) | Some(Wk::SCRIPT) => SyntaxKind::StringLiteral,
                _ => continue,
            },
            _ => continue,
        };
        push_token(&mut out, lines, &token, kind);
    }
    out
}

/// Push a highlight occurrence covering a rowan token, if non-empty.
fn push_token<L: rowan::Language>(
    out: &mut Vec<Occurrence>,
    lines: &LineTable,
    token: &rowan::SyntaxToken<L>,
    kind: SyntaxKind,
) {
    let r = token.text_range();
    let start: u32 = r.start().into();
    let end: u32 = r.end().into();
    if end > start {
        out.push(occurrence(lines, start, end, kind));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether any occurrence carries the given syntax kind. Compares the
    /// stored `EnumOrUnknown` directly because scip's `SyntaxKind` uses aliased
    /// discriminants, so `as i32` is unreliable.
    fn has_kind(occs: &[Occurrence], kind: SyntaxKind) -> bool {
        let want = kind.into();
        occs.iter().any(|o| o.syntax_kind == want)
    }

    #[test]
    fn deb822_highlights_comment_key_and_value() {
        let text = "# a comment\nSource: hello\n";
        let parsed = deb822_lossless::Deb822::parse(text);
        let lines = LineTable::new(text);
        let occs = deb822(&parsed.tree(), &lines);

        assert!(has_kind(&occs, SyntaxKind::Comment));
        assert!(has_kind(&occs, SyntaxKind::IdentifierAttribute));
        assert!(has_kind(&occs, SyntaxKind::StringLiteral));

        // Every highlight occurrence is symbol-less.
        assert!(occs.iter().all(|o| o.symbol.is_empty()));
    }

    #[test]
    fn makefile_highlights_comment_target_and_variable() {
        let text = "# c\nFOO = 1\nclean:\n\techo $(FOO)\n";
        let (mk, _) = makefile_lossless::Makefile::from_str_relaxed(text);
        let lines = LineTable::new(text);
        let occs = makefile(&mk, &lines);

        assert!(has_kind(&occs, SyntaxKind::Comment));
        assert!(has_kind(&occs, SyntaxKind::IdentifierFunction)); // target
        assert!(has_kind(&occs, SyntaxKind::IdentifierMutableGlobal)); // variable
        assert!(occs.iter().all(|o| o.symbol.is_empty()));
    }

    #[test]
    fn series_highlights_name_option_and_comment() {
        let text = "# security\nfix-arm.patch -p1\n";
        let parsed = patchkit::edit::series::parse(text);
        let lines = LineTable::new(text);
        let occs = series(&parsed.tree(), &lines);

        assert!(has_kind(&occs, SyntaxKind::IdentifierConstant)); // patch name
        assert!(has_kind(&occs, SyntaxKind::IdentifierParameter)); // -p1
        assert!(has_kind(&occs, SyntaxKind::Comment));
        assert!(occs.iter().all(|o| o.symbol.is_empty()));
    }

    #[test]
    fn changelog_highlights_version_package_and_comment() {
        let text = "hello (1.0-1) unstable; urgency=medium\n\n  * Change.\n\n -- T <t@example.org>  Tue, 27 May 2026 12:00:00 +0000\n";
        let cl = debian_changelog::ChangeLog::parse_relaxed(text);
        let lines = LineTable::new(text);
        let occs = changelog(&cl, &lines);

        assert!(has_kind(&occs, SyntaxKind::IdentifierConstant)); // version
        assert!(has_kind(&occs, SyntaxKind::IdentifierNamespace)); // package name
        assert!(occs.iter().all(|o| o.symbol.is_empty()));
    }

    #[test]
    fn watch_highlights_linebased_key_url_and_pattern() {
        let text = "version=4\nhttps://example.org/hello/ hello-(.+)\\.tar\\.gz\n";
        let lines = LineTable::new(text);
        let occs = watch(text, &lines);

        assert!(has_kind(&occs, SyntaxKind::IdentifierAttribute)); // version key
        assert!(has_kind(&occs, SyntaxKind::StringLiteral)); // URL
        assert!(occs.iter().all(|o| o.symbol.is_empty()));
    }

    #[test]
    fn diff_highlights_markers_hunks_and_changes() {
        let text = "--- a/foo.c\n+++ b/foo.c\n@@ -1,3 +1,3 @@\n context\n-removed\n+added\n more\n";
        let lines = LineTable::new(text);
        let occs = diff(text, 0, &lines);

        // File markers and the hunk header are highlighted.
        assert!(has_kind(&occs, SyntaxKind::IdentifierNamespace)); // ---/+++
        assert!(has_kind(&occs, SyntaxKind::IdentifierFunction)); // @@ header
                                                                  // Removed -> Comment, inserted -> StringLiteral.
        assert!(has_kind(&occs, SyntaxKind::Comment)); // -removed
        assert!(has_kind(&occs, SyntaxKind::StringLiteral)); // +added
        assert!(occs.iter().all(|o| o.symbol.is_empty()));

        // The `---` marker covers the whole line (offsets 0..11), no newline.
        let marker = occs
            .iter()
            .find(|o| o.range == vec![0, 0, 0, 11])
            .expect("--- marker occurrence");
        assert_eq!(marker.syntax_kind, SyntaxKind::IdentifierNamespace.into());
    }

    #[test]
    fn diff_offsets_are_relative_to_base() {
        // A header precedes the body; the body starts at `base`.
        let full = "Author: a\n\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-x\n+y\n";
        let base = full.find("--- ").unwrap() as u32;
        let lines = LineTable::new(full);
        let occs = diff(&full[base as usize..], base, &lines);

        // The `--- a/x` marker sits on line 2 (zero-indexed) of the full file.
        assert!(occs.iter().any(|o| o.range == vec![2, 0, 2, 7]));
    }

    #[test]
    fn diff_highlights_no_newline_marker_as_comment() {
        let text = "--- a/f\n+++ b/f\n@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+b\n";
        let lines = LineTable::new(text);
        let occs = diff(text, 0, &lines);
        // The "\ No newline" marker is its own node, highlighted as a comment.
        let nl_line = text.lines().position(|l| l.starts_with('\\')).unwrap() as i32;
        let marker = occs
            .iter()
            .find(|o| o.range[0] == nl_line)
            .expect("no-newline marker occurrence");
        assert_eq!(marker.syntax_kind, SyntaxKind::Comment.into());
        // The addition after it is still highlighted distinctly.
        assert!(has_kind(&occs, SyntaxKind::StringLiteral)); // +b
    }

    #[test]
    fn watch_highlights_v5_deb822() {
        let text = "Version: 5\n\nSource: https://example.org/hello/\nMatching-Pattern: hello-(.+)\\.tar\\.gz\n";
        let lines = LineTable::new(text);
        let occs = watch(text, &lines);

        // v5 routes through the deb822 highlighter: keys and values.
        assert!(has_kind(&occs, SyntaxKind::IdentifierAttribute)); // field names
        assert!(has_kind(&occs, SyntaxKind::StringLiteral)); // field values
        assert!(occs.iter().all(|o| o.symbol.is_empty()));
    }
}
