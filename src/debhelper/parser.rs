//! Shared lexer for the line-oriented debhelper config files.
//!
//! dirs, install, manpages, clean and friends share one grammar: a line is
//! either a `#` comment or a list of whitespace-separated path tokens, each of
//! which may embed `${...}` substitution variables. Decoding that grammar in
//! one place lets completion, semantic tokens, and diagnostics agree on token
//! boundaries instead of each helper re-slicing the raw string.
//!
//! All ranges are byte offsets into the line that was parsed.

use std::ops::Range;

/// A debhelper config line broken into its lexical pieces.
#[derive(Debug, PartialEq, Eq)]
pub struct Line {
    /// Range of the comment, including the leading `#`, when the whole line is
    /// a comment. `None` for a path line.
    pub comment: Option<Range<usize>>,
    /// The path tokens, in order. Empty for a blank or comment line.
    pub words: Vec<Word>,
}

/// A single whitespace-separated path token.
#[derive(Debug, PartialEq, Eq)]
pub struct Word {
    /// Range of the whole token within the line.
    pub range: Range<usize>,
    /// Literal and substitution pieces, in order.
    pub parts: Vec<Part>,
}

/// A piece of a token: plain text or a `${...}` substitution.
#[derive(Debug, PartialEq, Eq)]
pub enum Part {
    Literal(Range<usize>),
    Substitution(Substitution),
}

/// A `${...}` substitution variable inside a token.
#[derive(Debug, PartialEq, Eq)]
pub struct Substitution {
    /// Range of the whole substitution, including `${` and the closing `}`.
    pub range: Range<usize>,
    /// Range of the variable name between the braces. Empty for `${`.
    pub name: Range<usize>,
    /// Whether the closing `}` was present. An unterminated `${` runs to the
    /// end of the token.
    pub terminated: bool,
}

/// Parse a single line into its lexical pieces.
pub fn parse_line(line: &str) -> Line {
    // A line whose first non-whitespace character is `#` is a comment.
    if let Some(hash) = line.find(|c: char| !c.is_whitespace()) {
        if line[hash..].starts_with('#') {
            return Line {
                comment: Some(hash..line.len()),
                words: Vec::new(),
            };
        }
    }

    let words = word_ranges(line)
        .into_iter()
        .map(|range| parse_word(line, range))
        .collect();
    Line {
        comment: None,
        words,
    }
}

/// The byte range of each whitespace-separated run in the line.
///
/// `${Space}` carries no literal whitespace, so splitting the raw line never
/// cuts a token in half.
fn word_ranges(line: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = None;
    for (i, c) in line.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                ranges.push(s..i);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    // The trailing token has no whitespace to flush it.
    if let Some(s) = start {
        ranges.push(s..line.len());
    }
    ranges
}

/// Break a single token into its literal and substitution parts.
fn parse_word(line: &str, range: Range<usize>) -> Word {
    let base = range.start;
    let text = &line[range.clone()];
    let mut parts = Vec::new();
    let mut cursor = 0;

    while let Some(rel) = text[cursor..].find("${") {
        let open = cursor + rel;
        if open > cursor {
            parts.push(Part::Literal(base + cursor..base + open));
        }

        let name_start = open + 2;
        let (name_end, end, terminated) = match text[name_start..].find('}') {
            Some(rel) => (name_start + rel, name_start + rel + 1, true),
            None => (text.len(), text.len(), false),
        };
        parts.push(Part::Substitution(Substitution {
            range: base + open..base + end,
            name: base + name_start..base + name_end,
            terminated,
        }));

        cursor = end;
        if !terminated {
            break;
        }
    }

    if cursor < text.len() {
        parts.push(Part::Literal(base + cursor..base + text.len()));
    }
    Word { range, parts }
}

/// An open substitution immediately to the left of the cursor.
#[derive(Debug, PartialEq, Eq)]
pub enum SubstitutionStart {
    /// Cursor sits right after a bare `$`.
    Dollar,
    /// Cursor sits right after `${`.
    Brace,
}

/// What the cursor is positioned on within a debhelper config line.
///
/// This is the query completion needs: whether the cursor is in a comment, the
/// substitution it may be opening, which whitespace-separated token it is in,
/// and the text of that token up to the cursor. Per-helper meaning of a token
/// index (install's source vs destination, manpages' name-and-section) stays
/// with the helper.
#[derive(Debug, PartialEq, Eq)]
pub struct CursorContext<'a> {
    pub in_comment: bool,
    pub substitution: Option<SubstitutionStart>,
    /// Zero-based index of the token the cursor is in or about to begin.
    pub token_index: usize,
    /// Text of that token from its start up to the cursor.
    pub prefix: &'a str,
}

impl<'a> CursorContext<'a> {
    /// Describe the cursor sitting at `offset` bytes into `line`.
    pub fn at(line: &'a str, offset: usize) -> Self {
        let offset = offset.min(line.len());
        let before = &line[..offset];
        let substitution = if before.ends_with("${") {
            Some(SubstitutionStart::Brace)
        } else if before.ends_with('$') {
            Some(SubstitutionStart::Dollar)
        } else {
            None
        };

        let parsed = parse_line(line);
        let (token_index, prefix) = locate_token(line, &parsed.words, offset);

        CursorContext {
            in_comment: parsed.comment.is_some(),
            substitution,
            token_index,
            prefix,
        }
    }
}

/// Find which token the cursor is in or about to start, plus that token's text
/// up to the cursor. A cursor in whitespace begins the next token with an empty
/// prefix.
fn locate_token<'a>(line: &'a str, words: &[Word], offset: usize) -> (usize, &'a str) {
    for (index, word) in words.iter().enumerate() {
        if offset < word.range.start {
            // The cursor sits in the whitespace before this token.
            return (index, "");
        }
        if offset <= word.range.end {
            return (index, &line[word.range.start..offset]);
        }
    }
    // Past the last token: a fresh token after any trailing whitespace.
    (words.len(), "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_plain_path() {
        let parsed = parse_line("usr/bin/");
        assert_eq!(parsed.comment, None);
        assert_eq!(parsed.words.len(), 1);
        assert_eq!(parsed.words[0].range, 0..8);
        assert_eq!(parsed.words[0].parts, vec![Part::Literal(0..8)]);
    }

    #[test]
    fn splits_source_and_destination() {
        let parsed = parse_line("my-prog usr/bin/");
        let ranges: Vec<_> = parsed.words.iter().map(|w| w.range.clone()).collect();
        assert_eq!(ranges, vec![0..7, 8..16]);
    }

    #[test]
    fn ignores_leading_and_trailing_whitespace() {
        let parsed = parse_line("  usr/bin/  ");
        assert_eq!(parsed.words.len(), 1);
        assert_eq!(parsed.words[0].range, 2..10);
    }

    #[test]
    fn detects_a_comment_line() {
        let parsed = parse_line("   # install my-prog");
        assert_eq!(parsed.comment, Some(3..20));
        assert!(parsed.words.is_empty());
    }

    #[test]
    fn finds_a_substitution_inside_a_token() {
        let parsed = parse_line("usr/lib/${DEB_HOST_MULTIARCH}/foo");
        let parts = &parsed.words[0].parts;
        assert_eq!(parts[0], Part::Literal(0..8));
        assert_eq!(
            parts[1],
            Part::Substitution(Substitution {
                range: 8..29,
                name: 10..28,
                terminated: true,
            })
        );
        assert_eq!(parts[2], Part::Literal(29..33));
    }

    #[test]
    fn flags_an_unterminated_substitution() {
        let parsed = parse_line("usr/lib/${DEB_HOST");
        let parts = &parsed.words[0].parts;
        assert_eq!(
            parts[1],
            Part::Substitution(Substitution {
                range: 8..18,
                name: 10..18,
                terminated: false,
            })
        );
    }

    #[test]
    fn a_bare_dollar_is_literal() {
        let parsed = parse_line("usr/lib/$");
        assert_eq!(parsed.words[0].parts, vec![Part::Literal(0..9)]);
    }

    #[test]
    fn space_substitution_does_not_split_the_token() {
        // ${Space} holds no real whitespace, so it stays one token.
        let parsed = parse_line("a${Space}b");
        assert_eq!(parsed.words.len(), 1);
        assert_eq!(parsed.words[0].range, 0..10);
    }

    // Cursor queries, exercised against each helper's real needs.

    #[test]
    fn cursor_in_first_token_reports_index_zero() {
        let cx = CursorContext::at("my-pr", 5);
        assert_eq!(cx.token_index, 0);
        assert_eq!(cx.prefix, "my-pr");
        assert!(!cx.in_comment);
        assert_eq!(cx.substitution, None);
    }

    #[test]
    fn cursor_after_a_space_starts_the_next_token() {
        let cx = CursorContext::at("my-prog ", 8);
        assert_eq!(cx.token_index, 1);
        assert_eq!(cx.prefix, "");
    }

    #[test]
    fn cursor_in_destination_carries_its_prefix() {
        let cx = CursorContext::at("my-prog usr/", 12);
        assert_eq!(cx.token_index, 1);
        assert_eq!(cx.prefix, "usr/");
    }

    #[test]
    fn cursor_on_a_third_token_reports_index_two() {
        let cx = CursorContext::at("my-prog usr/bin ", 16);
        assert_eq!(cx.token_index, 2);
        assert_eq!(cx.prefix, "");
    }

    #[test]
    fn cursor_on_empty_line_is_token_zero() {
        let cx = CursorContext::at("", 0);
        assert_eq!(cx.token_index, 0);
        assert_eq!(cx.prefix, "");
    }

    #[test]
    fn cursor_reports_the_manpages_name_prefix() {
        let cx = CursorContext::at("foo.", 4);
        assert_eq!(cx.token_index, 0);
        assert_eq!(cx.prefix, "foo.");
    }

    #[test]
    fn cursor_in_a_comment_is_flagged() {
        let cx = CursorContext::at("# usr/bin", 9);
        assert!(cx.in_comment);
    }

    #[test]
    fn cursor_after_dollar_reports_a_bare_substitution() {
        let cx = CursorContext::at("usr/lib/$", 9);
        assert_eq!(cx.substitution, Some(SubstitutionStart::Dollar));
    }

    #[test]
    fn cursor_after_dollar_brace_reports_a_braced_substitution() {
        let cx = CursorContext::at("usr/lib/${", 10);
        assert_eq!(cx.substitution, Some(SubstitutionStart::Brace));
    }
}
