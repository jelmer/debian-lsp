//! Spell-checking for comment tokens in any rowan-based packaging file.
//!
//! Nearly every format the LSP understands exposes its comments as a single
//! token kind in a lossless syntax tree. This module walks such a tree, runs
//! the generic [`check_text`](super::check_text) over each comment token, and
//! maps the findings back to source ranges. A comment token's `text_range()`
//! is already in source coordinates, so a typo's byte offset within the token
//! text adds directly onto the token start; no continuation-line joining is
//! involved, unlike [`crate::control::spelling`].
//!
//! Callers supply a predicate over the format's own `SyntaxKind` to identify
//! comment tokens, keeping this module independent of any one parser crate.

use rowan::{Language, SyntaxNode, TextRange, TextSize};

use super::LocatedFinding;
use crate::position::Source;

/// Find spelling mistakes in every comment token of a syntax tree.
///
/// `is_comment` decides which token kinds hold comment prose; only those are
/// checked. Findings are mapped to LSP ranges via `src`.
pub fn comment_findings<L: Language>(
    root: &SyntaxNode<L>,
    src: Source<'_>,
    is_comment: impl Fn(L::Kind) -> bool,
) -> Vec<LocatedFinding> {
    let mut findings = Vec::new();

    for element in root.descendants_with_tokens() {
        let Some(token) = element.into_token() else {
            continue;
        };
        if !is_comment(token.kind()) {
            continue;
        }

        let token_start = token.text_range().start();
        for finding in super::check_text(token.text()) {
            let span = finding.span();
            let start = token_start + TextSize::new(span.start as u32);
            let end = token_start + TextSize::new(span.end as u32);
            let range = src.text_range_to_lsp_range(TextRange::new(start, end));
            findings.push(LocatedFinding { range, finding });
        }
    }

    findings
}

/// Comment findings for a deb822-based file (control, copyright, dep3 headers,
/// tests/control, source/options).
pub fn deb822_comment_findings(
    deb822: &deb822_lossless::Deb822,
    src: Source<'_>,
) -> Vec<LocatedFinding> {
    use deb822_lossless::SyntaxKind;
    use rowan::ast::AstNode;
    comment_findings(deb822.syntax(), src, |k| k == SyntaxKind::COMMENT)
}

/// Comment findings for a `debian/rules` makefile.
pub fn rules_comment_findings(
    makefile: &makefile_lossless::Makefile,
    src: Source<'_>,
) -> Vec<LocatedFinding> {
    use makefile_lossless::SyntaxKind;
    use rowan::ast::AstNode;
    comment_findings(makefile.syntax(), src, |k| k == SyntaxKind::COMMENT)
}

/// Comment findings for a `debian/changelog`.
pub fn changelog_comment_findings(
    parse: &debian_changelog::Parse<debian_changelog::ChangeLog>,
    src: Source<'_>,
) -> Vec<LocatedFinding> {
    use debian_changelog::SyntaxKind;
    // `syntax_node()` yields tokens even when the changelog has parse errors.
    comment_findings(&parse.syntax_node(), src, |k| k == SyntaxKind::COMMENT)
}

/// Comment findings for a `debian/<pkg>.lintian-overrides`.
pub fn lintian_overrides_comment_findings(
    overrides: &lintian_overrides::LintianOverrides,
    src: Source<'_>,
) -> Vec<LocatedFinding> {
    use lintian_overrides::{AstNode as _, SyntaxKind};
    comment_findings(overrides.syntax(), src, |k| k == SyntaxKind::COMMENT)
}

/// Comment findings for a `debian/patches/series` file. The comment text after
/// a `#` is a `TEXT` token; the `#` itself (`HASH`) carries no prose.
pub fn patches_series_comment_findings(
    series: &patchkit::edit::series::lossless::SeriesFile,
    src: Source<'_>,
) -> Vec<LocatedFinding> {
    use patchkit::edit::series::lex::SyntaxKind;
    use rowan::ast::AstNode;
    comment_findings(series.syntax(), src, |k| k == SyntaxKind::TEXT)
}

/// Comment findings for a `debian/watch` file, covering both the deb822 (v5)
/// and line-based (v1-4) representations. The cached parse is reused as-is.
pub fn watch_comment_findings(
    parse: &debian_watch::parse::Parse,
    src: Source<'_>,
) -> Vec<LocatedFinding> {
    match parse.to_watch_file() {
        debian_watch::parse::ParsedWatchFile::Deb822(wf) => {
            deb822_comment_findings(wf.as_deb822(), src)
        }
        debian_watch::parse::ParsedWatchFile::LineBased(wf) => {
            use debian_watch::SyntaxKind;
            comment_findings(wf.syntax(), src, |k| k == SyntaxKind::COMMENT)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;
    use tower_lsp_server::ls_types::{Position, Range};

    /// Build a workspace with one file and return its bits for the per-format
    /// finder. The parse trees pulled below are the salsa-cached ASTs, the
    /// same ones the LSP serves diagnostics from.
    fn setup(path: &str, content: &str) -> (Workspace, crate::workspace::SourceFile) {
        let mut workspace = Workspace::new();
        let url = str::parse(path).unwrap();
        let file = workspace.update_file(url, content.to_string());
        (workspace, file)
    }

    fn assert_single(found: &[LocatedFinding], typo: &str, correction: &str, range: Range) {
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].finding.typo, typo);
        assert_eq!(found[0].finding.corrections, vec![correction.to_string()]);
        assert_eq!(found[0].range, range);
    }

    #[test]
    fn test_rules_comment() {
        let content = "# This is a libary comment\nbuild:\n\techo hi\n";
        let (ws, file) = setup("file:///debian/rules", content);
        let text = ws.source_text(file);
        let idx = ws.get_line_index(file);
        let src = Source::new(&text, &idx);
        let found = rules_comment_findings(&ws.get_parsed_rules(file).tree(), src);
        // "libary" starts at column 12 of line 0.
        assert_single(
            &found,
            "libary",
            "library",
            Range::new(Position::new(0, 12), Position::new(0, 18)),
        );
    }

    #[test]
    fn test_rules_ignores_non_comment_tokens() {
        // A typo-shaped target name must not be flagged: it is not a comment.
        let content = "libary:\n\techo hi\n";
        let (ws, file) = setup("file:///debian/rules", content);
        let text = ws.source_text(file);
        let idx = ws.get_line_index(file);
        let src = Source::new(&text, &idx);
        assert!(rules_comment_findings(&ws.get_parsed_rules(file).tree(), src).is_empty());
    }

    #[test]
    fn test_deb822_comment() {
        let content = "# a libary comment\nSource: foo\n";
        let (ws, file) = setup("file:///debian/control", content);
        let text = ws.source_text(file);
        let idx = ws.get_line_index(file);
        let src = Source::new(&text, &idx);
        let found = deb822_comment_findings(&ws.get_parsed_deb822(file).tree(), src);
        assert_single(
            &found,
            "libary",
            "library",
            Range::new(Position::new(0, 4), Position::new(0, 10)),
        );
    }

    #[test]
    fn test_lintian_overrides_comment() {
        let content = "# overide this tag\nfoo: some-tag\n";
        let (ws, file) = setup("file:///debian/source/lintian-overrides", content);
        let text = ws.source_text(file);
        let idx = ws.get_line_index(file);
        let src = Source::new(&text, &idx);
        let found =
            lintian_overrides_comment_findings(&ws.get_parsed_lintian_overrides(file).tree(), src);
        assert_single(
            &found,
            "overide",
            "override",
            Range::new(Position::new(0, 2), Position::new(0, 9)),
        );
    }

    #[test]
    fn test_patches_series_comment() {
        let content = "# a libary patch\nfoo.patch\n";
        let (ws, file) = setup("file:///debian/patches/series", content);
        let text = ws.source_text(file);
        let idx = ws.get_line_index(file);
        let src = Source::new(&text, &idx);
        let found =
            patches_series_comment_findings(&ws.get_parsed_patches_series(file).tree(), src);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].finding.typo, "libary");
    }

    #[test]
    fn test_changelog_comment() {
        // A changelog comment is a '#' line at column 0 (an indented '#' is
        // ordinary entry body, not a comment).
        let content = "# a libary note\nfoo (1.0-1) unstable; urgency=low\n\n  * Initial release.\n\n -- Foo <foo@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n";
        let (ws, file) = setup("file:///debian/changelog", content);
        let text = ws.source_text(file);
        let idx = ws.get_line_index(file);
        let src = Source::new(&text, &idx);
        let found = changelog_comment_findings(&ws.get_parsed_changelog(file), src);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].finding.typo, "libary");
    }

    #[test]
    fn test_watch_linebased_comment() {
        let content = "# a libary comment\nversion=4\nhttps://example.com/foo-(.+).tar.gz\n";
        let (ws, file) = setup("file:///debian/watch", content);
        let text = ws.source_text(file);
        let idx = ws.get_line_index(file);
        let src = Source::new(&text, &idx);
        let found = watch_comment_findings(&ws.get_parsed_watch(file), src);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].finding.typo, "libary");
    }
}
