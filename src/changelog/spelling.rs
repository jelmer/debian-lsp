//! Spell-checking for the change descriptions of the latest `debian/changelog`
//! entry, while it is still unreleased.
//!
//! Only the topmost (most recent) entry is checked, and only when it is
//! unreleased: released entries are historical record and shouldn't sprout new
//! squiggles, but the entry being drafted for the next upload benefits from the
//! same prose checks as `debian/control`. Each DETAIL line (the `* ...` change
//! descriptions) is checked individually, so version strings, maintainer names
//! and timestamps in the header and footer are never flagged.

use debian_changelog::SyntaxKind;
use rowan::ast::AstNode;
use tower_lsp_server::ls_types::{CodeActionOrCommand, Diagnostic, Uri};

use crate::position::Source;
use crate::spelling::{check_text, make_actions, make_diagnostic, LocatedFinding};

/// Find spelling mistakes in the change descriptions of the latest changelog
/// entry, mapped to source ranges. Returns nothing unless the latest entry is
/// unreleased.
fn changelog_findings(
    parsed: &debian_changelog::Parse<debian_changelog::ChangeLog>,
    src: Source<'_>,
) -> Vec<LocatedFinding> {
    let changelog = parsed.tree();

    // The topmost entry is the most recent one; only check it while it is the
    // unreleased entry being drafted.
    let Some(entry) = changelog.iter().next() else {
        return Vec::new();
    };
    if entry.is_unreleased() != Some(true) {
        return Vec::new();
    }

    let mut findings = Vec::new();

    for element in entry.syntax().children() {
        if element.kind() != SyntaxKind::ENTRY_BODY {
            continue;
        }
        for child in element.descendants_with_tokens() {
            let rowan::NodeOrToken::Token(token) = child else {
                continue;
            };
            if token.kind() != SyntaxKind::DETAIL {
                continue;
            }

            let token_start = token.text_range().start();
            for finding in check_text(token.text()) {
                // DETAIL tokens are contiguous in the source, so the typo span
                // is a simple shift from the token start.
                let span = finding.span();
                let start = token_start + text_size::TextSize::from(span.start as u32);
                let end = token_start + text_size::TextSize::from(span.end as u32);
                let range = src.text_range_to_lsp_range(text_size::TextRange::new(start, end));
                findings.push(LocatedFinding { range, finding });
            }
        }
    }

    findings
}

/// Produce spelling diagnostics for the latest unreleased changelog entry.
pub fn changelog_diagnostics(
    parsed: &debian_changelog::Parse<debian_changelog::ChangeLog>,
    src: Source<'_>,
) -> Vec<Diagnostic> {
    changelog_findings(parsed, src)
        .into_iter()
        .map(|located| make_diagnostic(located.range, &located.finding))
        .collect()
}

/// Produce quick-fix code actions for spelling mistakes in the latest
/// unreleased changelog entry. One action per suggested correction.
///
/// When `diagnostics` is non-empty (the client requested fixes for specific
/// diagnostics), only actions whose range matches one of them are emitted, so
/// the quickfix doesn't appear for every nearby squiggle.
pub fn changelog_actions(
    uri: &Uri,
    parsed: &debian_changelog::Parse<debian_changelog::ChangeLog>,
    src: Source<'_>,
    diagnostics: &[Diagnostic],
) -> Vec<CodeActionOrCommand> {
    make_actions(uri, changelog_findings(parsed, src), diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;
    use tower_lsp_server::ls_types::{CodeAction, CodeActionKind, NumberOrString, Position, Range};

    fn changelog_diags(content: &str) -> Vec<Diagnostic> {
        let mut workspace = Workspace::new();
        let url = str::parse("file:///debian/changelog").unwrap();
        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_changelog(file);
        let text = workspace.source_text(file);
        let idx = workspace.get_line_index(file);
        let src = Source::new(&text, &idx);
        changelog_diagnostics(&parsed, src)
    }

    const UNRELEASED: &str = "foo (1.0-1) UNRELEASED; urgency=medium\n\n  * Initial release with a libary typo.\n\n -- Foo <foo@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n";

    #[test]
    fn test_changelog_diagnostics_unreleased_typo() {
        let diags = changelog_diags(UNRELEASED);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "\"libary\" should be \"library\"");
        // "libary" sits on the change line (line 2).
        assert_eq!(diags[0].range.start.line, 2);
    }

    #[test]
    fn test_changelog_diagnostics_skips_released_entry() {
        // The same entry, but released: no spelling diagnostics.
        let released = "foo (1.0-1) unstable; urgency=medium\n\n  * Initial release with a libary typo.\n\n -- Foo <foo@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n";
        assert_eq!(changelog_diags(released), vec![]);
    }

    #[test]
    fn test_changelog_diagnostics_entry_without_signature() {
        // A freshly-started entry with no `-- maintainer` signature line yet
        // is still unreleased and gets checked.
        let content =
            "foo (1.0-1) UNRELEASED; urgency=medium\n\n  * Initial release with a libary typo.\n";
        let diags = changelog_diags(content);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "\"libary\" should be \"library\"");
    }

    #[test]
    fn test_changelog_diagnostics_only_latest_entry() {
        // A clean unreleased entry on top, a typo in an older released entry
        // below: nothing is flagged because only the latest entry is checked.
        let content = "foo (1.0-2) UNRELEASED; urgency=medium\n\n  * Clean change.\n\n -- Foo <foo@example.com>  Tue, 02 Jan 2024 00:00:00 +0000\n\nfoo (1.0-1) unstable; urgency=medium\n\n  * Initial release with a libary typo.\n\n -- Foo <foo@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n";
        assert_eq!(changelog_diags(content), vec![]);
    }

    #[test]
    fn test_changelog_diagnostics_skips_header_and_footer() {
        // A typo-shaped token in the package name or maintainer line must not
        // be flagged; only DETAIL change lines are checked.
        let content = "libary (1.0-1) UNRELEASED; urgency=medium\n\n  * Clean change.\n\n -- Teh Author <foo@example.com>  Mon, 01 Jan 2024 00:00:00 +0000\n";
        assert_eq!(changelog_diags(content), vec![]);
    }

    fn changelog_acts(content: &str, diagnostics: &[Diagnostic]) -> Vec<CodeAction> {
        let mut workspace = Workspace::new();
        let url: Uri = str::parse("file:///debian/changelog").unwrap();
        let file = workspace.update_file(url.clone(), content.to_string());
        let parsed = workspace.get_parsed_changelog(file);
        let text = workspace.source_text(file);
        let idx = workspace.get_line_index(file);
        let src = Source::new(&text, &idx);
        changelog_actions(&url, &parsed, src, diagnostics)
            .into_iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(action) => action,
                CodeActionOrCommand::Command(_) => panic!("expected CodeAction"),
            })
            .collect()
    }

    #[test]
    fn test_changelog_actions_quickfix() {
        let actions = changelog_acts(UNRELEASED, &[]);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Change \"libary\" to \"library\"");
        assert_eq!(actions[0].kind, Some(CodeActionKind::QUICKFIX));

        let edits = actions[0]
            .edit
            .as_ref()
            .and_then(|e| e.changes.as_ref())
            .and_then(|c| c.values().next())
            .expect("action should carry a text edit");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "library");
    }

    #[test]
    fn test_changelog_actions_filter_by_diagnostic() {
        let diags = changelog_diags(UNRELEASED);
        assert_eq!(diags.len(), 1);
        let typo_range = diags[0].range;

        let unrelated = Diagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 1)),
            code: Some(NumberOrString::String("spelling".to_string())),
            ..Default::default()
        };
        assert_eq!(
            changelog_acts(UNRELEASED, std::slice::from_ref(&unrelated)),
            vec![]
        );

        let matching = Diagnostic {
            range: typo_range,
            code: Some(NumberOrString::String("spelling".to_string())),
            ..Default::default()
        };
        let actions = changelog_acts(UNRELEASED, std::slice::from_ref(&matching));
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].diagnostics, Some(vec![matching]));
    }
}
