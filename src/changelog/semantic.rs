//! Semantic token generation for Debian changelog files.

use debian_changelog::SyntaxKind;
use tower_lsp_server::ls_types::SemanticToken;

use crate::deb822::semantic::{SemanticTokensBuilder, TokenType};
use crate::position::offset_to_position;

/// Generate semantic tokens for a changelog file
pub fn generate_semantic_tokens(
    parse: &debian_changelog::Parse<debian_changelog::ChangeLog>,
    source_text: &str,
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
                        push_token(&mut builder, source_text, range.start(), token.text(), tt);
                    }
                }
                SyntaxKind::VERSION => {
                    push_token(
                        &mut builder,
                        source_text,
                        range.start(),
                        token.text(),
                        TokenType::ChangelogVersion,
                    );
                }
                SyntaxKind::COMMENT => {
                    push_token(
                        &mut builder,
                        source_text,
                        range.start(),
                        token.text(),
                        TokenType::Comment,
                    );
                }
                SyntaxKind::DETAIL => {
                    bug_ref_continues = push_bug_references(
                        &mut builder,
                        source_text,
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
                        push_token(&mut builder, source_text, range.start(), token.text(), tt);
                    }
                }
            }
        }
    }

    builder.build()
}

fn push_token(
    builder: &mut SemanticTokensBuilder,
    source_text: &str,
    start: text_size::TextSize,
    text: &str,
    token_type: TokenType,
) {
    let start_pos = offset_to_position(source_text, start);
    let length = crate::position::utf16_len(text);
    if length > 0 {
        builder.push(start_pos.line, start_pos.character, length, token_type, 0);
    }
}

/// Emit semantic tokens for bug references within a DETAIL token.
///
/// Highlights `Closes: #NNN, #NNN` and `LP: #NNN, #NNN` spans, including
/// references that wrap across DETAIL tokens (continuation lines).
///
/// Returns `true` if the reference continues past the end of this token
/// (the line ended with a trailing comma or with the marker but no digits).
fn push_bug_references(
    builder: &mut SemanticTokensBuilder,
    source_text: &str,
    token_start: text_size::TextSize,
    text: &str,
    continues_from_prev: bool,
) -> bool {
    let start: usize = token_start.into();
    let lower = text.to_ascii_lowercase();

    // If we're continuing from a previous line, highlight leading bug numbers.
    // A continuation line looks like "    #789012" or "    #789012, #345678".
    if continues_from_prev {
        if let Some(end) = highlight_continuation(builder, source_text, start, text) {
            // Check if this continuation itself continues (trailing comma).
            let highlighted = &text[..end];
            if highlighted.trim_end().ends_with(',') {
                return true;
            }
        }
    }

    // Scan for new marker occurrences in this token.
    let mut last_continues = false;
    for marker in &["closes:", "lp:"] {
        for (idx, _) in lower.match_indices(marker) {
            if idx > 0 {
                let prev = lower.as_bytes()[idx - 1];
                if prev.is_ascii_alphanumeric() || prev == b'-' || prev == b'_' {
                    continue;
                }
            }

            let after = &text[idx + marker.len()..];
            let span_end = idx
                + marker.len()
                + after
                    .find(|c: char| {
                        !(c.is_ascii_whitespace() || c == ',' || c == '#' || c.is_ascii_digit())
                    })
                    .unwrap_or(after.len());

            let content = &text[idx + marker.len()..span_end];

            // Trim trailing whitespace/commas from the highlighted span.
            let trimmed_end = idx
                + marker.len()
                + content
                    .trim_end_matches(|c: char| c == ',' || c.is_ascii_whitespace())
                    .len();

            let matched_text = &text[idx..trimmed_end];
            // Even if there are no digits yet (e.g. "Closes:" at end of line),
            // we still emit the marker if there's content; the continuation
            // will pick up the digits on the next line.
            let has_digits = content.chars().any(|c| c.is_ascii_digit());

            if has_digits {
                let abs_start = text_size::TextSize::from((start + idx) as u32);
                let start_pos = offset_to_position(source_text, abs_start);
                let length = crate::position::utf16_len(matched_text);
                if length > 0 {
                    builder.push(
                        start_pos.line,
                        start_pos.character,
                        length,
                        TokenType::ChangelogBugReference,
                        0,
                    );
                }
            }

            // Check if this reference continues to the next line:
            // either the marker has no digits (just "Closes:" at EOL),
            // or it ends with a trailing comma.
            let reaches_eol = span_end == text.len();
            if reaches_eol {
                let trimmed_content = content.trim();
                last_continues =
                    !has_digits || trimmed_content.ends_with(',') || trimmed_content.is_empty();
            }
        }
    }

    last_continues
}

/// Highlight continuation bug numbers at the start of a DETAIL token.
///
/// Returns the byte offset within `text` up to which we highlighted,
/// or `None` if this doesn't look like a continuation line.
fn highlight_continuation(
    builder: &mut SemanticTokensBuilder,
    source_text: &str,
    start: usize,
    text: &str,
) -> Option<usize> {
    // Continuation lines contain only whitespace, commas, hashes, and digits.
    // Find the extent of the bug-number portion.
    let end = text
        .find(|c: char| !(c.is_ascii_whitespace() || c == ',' || c == '#' || c.is_ascii_digit()))
        .unwrap_or(text.len());

    let span = &text[..end];
    if !span.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }

    // Trim leading/trailing whitespace for the highlighted region.
    let trimmed = span.trim();
    let trimmed = trimmed.trim_end_matches(|c: char| c == ',' || c.is_ascii_whitespace());
    if trimmed.is_empty() {
        return None;
    }

    let offset_in_text = text.find(trimmed).unwrap();
    let abs_start = text_size::TextSize::from((start + offset_in_text) as u32);
    let start_pos = offset_to_position(source_text, abs_start);
    let length = crate::position::utf16_len(trimmed);
    if length > 0 {
        builder.push(
            start_pos.line,
            start_pos.character,
            length,
            TokenType::ChangelogBugReference,
            0,
        );
    }

    Some(end)
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
        let tokens = generate_semantic_tokens(&parsed, text);

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
        let tokens = generate_semantic_tokens(&parsed, text);

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
        let tokens = generate_semantic_tokens(&parsed, text);

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
        let tokens = generate_semantic_tokens(&parsed, text);

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
        let tokens = generate_semantic_tokens(&parsed, text);

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
        let tokens = generate_semantic_tokens(&parsed, text);

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
        let tokens = generate_semantic_tokens(&parsed, text);

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
        let tokens = generate_semantic_tokens(&parsed, text);

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
        let tokens = generate_semantic_tokens(&parsed, text);

        let bug_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::ChangelogBugReference as u32)
            .collect();
        // Two tokens: "Closes:" on line 2, "#123456" on line 3
        assert_eq!(bug_tokens.len(), 1, "bug tokens: {bug_tokens:?}");
        // The continuation line should highlight "#123456" (7 chars)
        assert_eq!(bug_tokens[0].length, 7);
    }

    #[test]
    fn test_bug_reference_multiline_with_comma() {
        // Bugs split across lines with comma
        let text = "pkg (1.0-1) unstable; urgency=low\n\n  * Fix bugs. Closes: #111,\n    #222\n\n -- T <t@t.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = debian_changelog::ChangeLog::parse(text);
        let tokens = generate_semantic_tokens(&parsed, text);

        let bug_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.token_type == TokenType::ChangelogBugReference as u32)
            .collect();
        // Two tokens: "Closes: #111" on first line, "#222" on second
        assert_eq!(bug_tokens.len(), 2, "bug tokens: {bug_tokens:?}");
        assert_eq!(bug_tokens[0].length, 12); // "Closes: #111"
        assert_eq!(bug_tokens[1].length, 4); // "#222"
    }
}
