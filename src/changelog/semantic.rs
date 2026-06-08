//! Semantic token generation for Debian changelog files.

use debian_changelog::SyntaxKind;
use tower_lsp_server::ls_types::SemanticToken;

use crate::deb822::semantic::{SemanticTokensBuilder, TokenType};
use crate::position::Source;

/// Generate semantic tokens for a changelog file
pub fn generate_semantic_tokens(
    parse: &debian_changelog::Parse<debian_changelog::ChangeLog>,
    src: Source<'_>,
) -> Vec<SemanticToken> {
    let mut builder = SemanticTokensBuilder::new();

    // Use syntax_node() to get tokens even with parse errors
    let syntax = parse.syntax_node();

    // Track whether the previous DETAIL token ended inside a bug reference
    // (e.g. "Closes: #111,\n" or "Closes:\n"), so we can continue
    // highlighting on the next DETAIL line.
    let mut bug_ref_continues = false;

    for element in syntax.descendants_with_tokens() {
        if let rowan::NodeOrToken::Token(token) = element {
            let kind = token.kind();

            let range = token.text_range();

            match kind {
                SyntaxKind::IDENTIFIER => {
                    let parent_kind = token.parent().map(|p| p.kind());
                    let token_type = match parent_kind {
                        Some(SyntaxKind::ENTRY_HEADER) => Some(TokenType::ChangelogPackage),
                        Some(SyntaxKind::METADATA_KEY) => Some(TokenType::ChangelogUrgency),
                        Some(SyntaxKind::DISTRIBUTIONS) => Some(TokenType::ChangelogDistribution),
                        Some(SyntaxKind::METADATA_VALUE) => Some(TokenType::ChangelogMetadataValue),
                        _ => None,
                    };
                    if let Some(tt) = token_type {
                        push_token(&mut builder, src, range.start(), token.text(), tt);
                    }
                }
                SyntaxKind::VERSION => {
                    push_token(
                        &mut builder,
                        src,
                        range.start(),
                        token.text(),
                        TokenType::ChangelogVersion,
                    );
                }
                SyntaxKind::COMMENT => {
                    push_token(
                        &mut builder,
                        src,
                        range.start(),
                        token.text(),
                        TokenType::Comment,
                    );
                }
                SyntaxKind::DETAIL => {
                    bug_ref_continues = push_detail_references(
                        &mut builder,
                        src,
                        range.start(),
                        token.text(),
                        bug_ref_continues,
                    );
                }
                _ => {
                    let parent_kind = token.parent().map(|p| p.kind());
                    let token_type = match parent_kind {
                        Some(SyntaxKind::METADATA_VALUE) => Some(TokenType::ChangelogMetadataValue),
                        Some(SyntaxKind::TIMESTAMP) => Some(TokenType::ChangelogTimestamp),
                        Some(SyntaxKind::MAINTAINER) => Some(TokenType::ChangelogMaintainer),
                        Some(SyntaxKind::EMAIL) => Some(TokenType::ChangelogMaintainer),
                        _ => None,
                    };
                    if let Some(tt) = token_type {
                        push_token(&mut builder, src, range.start(), token.text(), tt);
                    }
                }
            }
        }
    }

    builder.build()
}

fn push_token(
    builder: &mut SemanticTokensBuilder,
    src: Source<'_>,
    start: text_size::TextSize,
    text: &str,
    token_type: TokenType,
) {
    let start_pos = src.offset_to_position(start);
    let length = crate::position::utf16_len(text);
    if length > 0 {
        builder.push(start_pos.line, start_pos.character, length, token_type, 0);
    }
}

/// Emit semantic tokens for bug and CVE references within a DETAIL token.
///
/// Highlights `Closes: #NNN, #NNN` and `LP: #NNN, #NNN` bug spans (including
/// references that wrap across DETAIL tokens) and `CVE-YYYY-NNNN` identifiers.
/// Spans are emitted in document order so the builder's deltas stay monotonic.
///
/// Returns `true` if a bug reference continues past the end of this token.
fn push_detail_references(
    builder: &mut SemanticTokensBuilder,
    src: Source<'_>,
    token_start: text_size::TextSize,
    text: &str,
    continues_from_prev: bool,
) -> bool {
    let start: usize = token_start.into();

    // Collect (relative-start, relative-end, token-type) for both kinds, then
    // sort by position before pushing.
    let mut spans: Vec<(usize, usize, TokenType)> = Vec::new();

    let bug_spans = debian_changelog::bugs::bug_ref_spans(text, continues_from_prev);
    let mut last_continues = false;
    for span in &bug_spans {
        spans.push((span.start, span.end, TokenType::ChangelogBugReference));
        if span.continues {
            last_continues = true;
        }
    }

    for c in crate::cve::find_cves(text) {
        spans.push((c.start, c.end, TokenType::ChangelogCve));
    }

    spans.sort_by_key(|(s, _, _)| *s);

    for (s, e, token_type) in spans {
        let abs_start = text_size::TextSize::from((start + s) as u32);
        let start_pos = src.offset_to_position(abs_start);
        let length = crate::position::utf16_len(&text[s..e]);
        if length > 0 {
            builder.push(start_pos.line, start_pos.character, length, token_type, 0);
        }
    }

    last_continues
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to collect (token_type, length) pairs for easier assertions
    fn token_summary(tokens: &[SemanticToken]) -> Vec<(u32, u32)> {
        tokens.iter().map(|t| (t.token_type, t.length)).collect()
    }

    #[test]
    fn test_all_token_types_in_entry() {
        let text = "test-package (1.0-1) unstable; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);
        let idx = crate::position::LineIndex::new(text);
        let tokens = generate_semantic_tokens(&parsed, Source::new(text, &idx));

        let summary = token_summary(&tokens);

        // Verify we see all expected token types
        let types: Vec<u32> = summary.iter().map(|(tt, _)| *tt).collect();

        assert!(
            types.contains(&(TokenType::ChangelogPackage as u32)),
            "Missing ChangelogPackage in {types:?}"
        );
        assert!(
            types.contains(&(TokenType::ChangelogVersion as u32)),
            "Missing ChangelogVersion in {types:?}"
        );
        assert!(
            types.contains(&(TokenType::ChangelogDistribution as u32)),
            "Missing ChangelogDistribution in {types:?}"
        );
        assert!(
            types.contains(&(TokenType::ChangelogUrgency as u32)),
            "Missing ChangelogUrgency in {types:?}"
        );
        assert!(
            types.contains(&(TokenType::ChangelogMetadataValue as u32)),
            "Missing Value (metadata value) in {types:?}"
        );
        assert!(
            types.contains(&(TokenType::ChangelogMaintainer as u32)),
            "Missing ChangelogMaintainer in {types:?}"
        );
        assert!(
            types.contains(&(TokenType::ChangelogTimestamp as u32)),
            "Missing ChangelogTimestamp in {types:?}"
        );

        // First token should be the package name
        assert_eq!(tokens[0].delta_line, 0);
        assert_eq!(tokens[0].delta_start, 0);
        assert_eq!(tokens[0].token_type, TokenType::ChangelogPackage as u32);
        assert_eq!(tokens[0].length, 12);
    }

    #[test]
    fn test_maintainer_and_timestamp() {
        let text = "pkg (1.0-1) unstable; urgency=low\n\n  * Change.\n\n -- Test User <test@test.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);
        let idx = crate::position::LineIndex::new(text);
        let tokens = generate_semantic_tokens(&parsed, Source::new(text, &idx));

        let summary = token_summary(&tokens);

        let has_maintainer = summary
            .iter()
            .any(|(tt, _)| *tt == TokenType::ChangelogMaintainer as u32);
        assert!(has_maintainer, "Should have a maintainer token");

        let has_timestamp = summary
            .iter()
            .any(|(tt, _)| *tt == TokenType::ChangelogTimestamp as u32);
        assert!(has_timestamp, "Should have a timestamp token");
    }

    #[test]
    fn test_multiple_entries() {
        let text = "\
pkg (2.0-1) unstable; urgency=medium

  * Second release.

 -- A <a@example.com>  Mon, 01 Jan 2025 12:00:00 +0000

pkg (1.0-1) unstable; urgency=low

  * First release.

 -- B <b@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
";
        let parsed = debian_changelog::ChangeLog::parse(text);
        let idx = crate::position::LineIndex::new(text);
        let tokens = generate_semantic_tokens(&parsed, Source::new(text, &idx));

        // Should have package tokens for both entries
        let package_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::ChangelogPackage as u32)
            .collect();
        assert_eq!(package_tokens.len(), 2, "Should have 2 package name tokens");

        // Should have version tokens for both entries
        let version_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::ChangelogVersion as u32)
            .collect();
        assert_eq!(version_tokens.len(), 2, "Should have 2 version tokens");
    }

    #[test]
    fn test_multiple_distributions() {
        let text = "pkg (1.0-1) unstable testing; urgency=low\n\n  * Change.\n\n -- T <t@t.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);
        let idx = crate::position::LineIndex::new(text);
        let tokens = generate_semantic_tokens(&parsed, Source::new(text, &idx));

        let dist_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::ChangelogDistribution as u32)
            .collect();
        assert_eq!(
            dist_tokens.len(),
            2,
            "Should have 2 distribution tokens for 'unstable testing'"
        );
    }

    #[test]
    fn test_bug_reference_closes() {
        let text = "pkg (1.0-1) unstable; urgency=low\n\n  * Fix bug. (Closes: #123456)\n\n -- T <t@t.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);
        let idx = crate::position::LineIndex::new(text);
        let tokens = generate_semantic_tokens(&parsed, Source::new(text, &idx));

        let bug_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::ChangelogBugReference as u32)
            .collect();
        assert_eq!(bug_tokens.len(), 1);
        // "Closes: #123456" is 15 chars
        assert_eq!(bug_tokens[0].length, 15);
    }

    #[test]
    fn test_bug_reference_lp() {
        let text = "pkg (1.0-1) unstable; urgency=low\n\n  * Fix bug. (LP: #987654)\n\n -- T <t@t.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);
        let idx = crate::position::LineIndex::new(text);
        let tokens = generate_semantic_tokens(&parsed, Source::new(text, &idx));

        let bug_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::ChangelogBugReference as u32)
            .collect();
        assert_eq!(bug_tokens.len(), 1);
        // "LP: #987654" is 11 chars
        assert_eq!(bug_tokens[0].length, 11);
    }

    #[test]
    fn test_bug_reference_multiple() {
        let text = "pkg (1.0-1) unstable; urgency=low\n\n  * Fix bugs. (Closes: #111, #222)\n\n -- T <t@t.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);
        let idx = crate::position::LineIndex::new(text);
        let tokens = generate_semantic_tokens(&parsed, Source::new(text, &idx));

        let bug_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::ChangelogBugReference as u32)
            .collect();
        // Single span covering "Closes: #111, #222"
        assert_eq!(bug_tokens.len(), 1);
        assert_eq!(bug_tokens[0].length, 18);
    }

    #[test]
    fn test_no_bug_reference() {
        let text = "pkg (1.0-1) unstable; urgency=low\n\n  * Regular change.\n\n -- T <t@t.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);
        let idx = crate::position::LineIndex::new(text);
        let tokens = generate_semantic_tokens(&parsed, Source::new(text, &idx));

        let bug_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::ChangelogBugReference as u32)
            .collect();
        assert_eq!(bug_tokens.len(), 0);
    }

    #[test]
    fn test_bug_reference_multiline() {
        // "Closes:" on one line, bug number on the next
        let text = "pkg (1.0-1) unstable; urgency=low\n\n  * Fix bug. Closes:\n    #123456\n\n -- T <t@t.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);
        let idx = crate::position::LineIndex::new(text);
        let tokens = generate_semantic_tokens(&parsed, Source::new(text, &idx));

        let bug_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::ChangelogBugReference as u32)
            .collect();
        // Two tokens: "Closes:" on line 2, "#123456" on line 3
        assert_eq!(bug_tokens.len(), 2, "bug tokens: {bug_tokens:?}");
        assert_eq!(bug_tokens[0].length, 7); // "Closes:"
        assert_eq!(bug_tokens[1].length, 7); // "#123456"
    }

    #[test]
    fn test_bug_reference_multiline_with_comma() {
        // Bugs split across lines with comma
        let text = "pkg (1.0-1) unstable; urgency=low\n\n  * Fix bugs. Closes: #111,\n    #222\n\n -- T <t@t.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);
        let idx = crate::position::LineIndex::new(text);
        let tokens = generate_semantic_tokens(&parsed, Source::new(text, &idx));

        let bug_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::ChangelogBugReference as u32)
            .collect();
        // Two tokens: "Closes: #111" on first line, "#222" on second
        assert_eq!(bug_tokens.len(), 2, "bug tokens: {bug_tokens:?}");
        assert_eq!(bug_tokens[0].length, 12); // "Closes: #111"
        assert_eq!(bug_tokens[1].length, 4); // "#222"
    }

    #[test]
    fn test_cve_reference() {
        let text = "pkg (1.0-1) unstable; urgency=low\n\n  * Fix CVE-2024-1234.\n\n -- T <t@t.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);
        let idx = crate::position::LineIndex::new(text);
        let tokens = generate_semantic_tokens(&parsed, Source::new(text, &idx));

        let cve_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::ChangelogCve as u32)
            .collect();
        assert_eq!(cve_tokens.len(), 1);
        assert_eq!(cve_tokens[0].length, 13); // "CVE-2024-1234"
    }

    #[test]
    fn test_cve_and_bug_in_same_detail_emit_in_order() {
        // A CVE before a Closes: in the same detail line. Tokens must be emitted
        // in document order so the builder's deltas stay monotonic.
        let text = "pkg (1.0-1) unstable; urgency=low\n\n  * Fix CVE-2024-1234 (Closes: #111)\n\n -- T <t@t.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);
        let idx = crate::position::LineIndex::new(text);
        let tokens = generate_semantic_tokens(&parsed, Source::new(text, &idx));

        let refs: Vec<_> = tokens
            .iter()
            .filter(|t| {
                t.token_type == TokenType::ChangelogCve as u32
                    || t.token_type == TokenType::ChangelogBugReference as u32
            })
            .collect();
        assert_eq!(refs.len(), 2);
        // CVE comes first in the source, so it is emitted first.
        assert_eq!(refs[0].token_type, TokenType::ChangelogCve as u32);
        assert_eq!(refs[1].token_type, TokenType::ChangelogBugReference as u32);
    }
}
