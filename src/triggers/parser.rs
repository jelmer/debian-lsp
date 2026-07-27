use std::ops::Range;

/// A triggers line broken into its lexical pieces.
#[derive(Debug, PartialEq, Eq)]
pub struct Line {
    /// Range of the comment, from the first `#` to the end of the line.
    pub comment: Option<Range<usize>>,
    /// The words before the comment, in order.
    pub words: Vec<Range<usize>>,
}

/// Parse a single line into its lexical pieces.
pub fn parse_line(line: &str) -> Line {
    let comment = line.find('#').map(|hash| hash..line.len());
    let code = match &comment {
        Some(comment) => &line[..comment.start],
        None => line,
    };

    let mut words = Vec::new();
    let mut start = None;
    for (offset, c) in code.char_indices() {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(offset),
            (true, Some(begin)) => {
                words.push(begin..offset);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(begin) = start {
        words.push(begin..code.len());
    }

    Line { comment, words }
}

/// Where the cursor sits on a triggers line.
pub struct CursorContext<'a> {
    pub in_comment: bool,
    /// Zero-based index of the word the cursor is in or about to begin.
    pub token_index: usize,
    /// Text of that word from its start up to the cursor.
    pub prefix: &'a str,
}

impl<'a> CursorContext<'a> {
    /// Describe the cursor sitting at `offset` bytes into `line`.
    pub fn at(line: &'a str, offset: usize) -> Self {
        let offset = offset.min(line.len());
        let parsed = parse_line(line);
        let in_comment = parsed.comment.iter().any(|c| offset > c.start);
        let (token_index, prefix) = locate_word(line, &parsed.words, offset);

        CursorContext {
            in_comment,
            token_index,
            prefix,
        }
    }
}

/// Find which word the cursor is in or about to start.
fn locate_word<'a>(line: &'a str, words: &[Range<usize>], offset: usize) -> (usize, &'a str) {
    for (index, word) in words.iter().enumerate() {
        if offset < word.start {
            return (index, "");
        }
        if offset <= word.end {
            return (index, &line[word.start..offset]);
        }
    }
    (words.len(), "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_directive_and_its_trigger() {
        let parsed = parse_line("interest /usr/share/foo");
        assert_eq!(parsed.comment, None);
        assert_eq!(parsed.words, vec![0..8, 9..23]);
    }

    #[test]
    fn ignores_leading_and_trailing_whitespace() {
        let parsed = parse_line("  interest  ");
        assert_eq!(parsed.words, vec![2..10]);
    }

    #[test]
    fn a_comment_may_follow_a_directive() {
        let parsed = parse_line("interest foo # why we care");
        assert_eq!(parsed.comment, Some(13..26));
        assert_eq!(parsed.words, vec![0..8, 9..12]);
    }

    #[test]
    fn a_whole_line_may_be_a_comment() {
        let parsed = parse_line("# nothing here");
        assert_eq!(parsed.comment, Some(0..14));
        assert!(parsed.words.is_empty());
    }

    #[test]
    fn a_blank_line_has_no_words() {
        assert!(parse_line("   ").words.is_empty());
    }

    #[test]
    fn cursor_inside_a_word_reports_its_prefix() {
        let cx = CursorContext::at("inter", 5);
        assert_eq!(cx.token_index, 0);
        assert_eq!(cx.prefix, "inter");
        assert!(!cx.in_comment);
    }

    #[test]
    fn cursor_after_a_word_begins_the_next_one() {
        let cx = CursorContext::at("interest ", 9);
        assert_eq!(cx.token_index, 1);
        assert_eq!(cx.prefix, "");
    }

    #[test]
    fn cursor_past_a_trailing_comment_is_in_it() {
        let cx = CursorContext::at("interest foo # why", 15);
        assert!(cx.in_comment);
    }

    #[test]
    fn cursor_before_a_trailing_comment_is_not_in_it() {
        let cx = CursorContext::at("interest foo # why", 12);
        assert!(!cx.in_comment);
    }
}
