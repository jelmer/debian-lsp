use tower_lsp_server::ls_types::SemanticToken;

use crate::deb822::semantic::{SemanticTokensBuilder, TokenType};
use crate::debhelper::parser::{parse_line, Part};
use crate::position::{utf16_len, Source};

/// Semantic tokens for a line-oriented debhelper file.
pub fn generate_semantic_tokens(src: Source<'_>) -> Vec<SemanticToken> {
    let mut builder = SemanticTokensBuilder::new();

    for (line_num, line) in src.text.lines().enumerate() {
        let line_num = line_num as u32;
        let parsed = parse_line(line);

        if let Some(comment) = parsed.comment {
            push(
                &mut builder,
                line,
                line_num,
                comment.start,
                comment.end,
                TokenType::Comment,
            );
            continue;
        }

        for word in &parsed.words {
            for part in &word.parts {
                let (range, token_type) = match part {
                    Part::Literal(range) => (range, TokenType::Value),
                    Part::Substitution(sub) => (&sub.range, TokenType::Field),
                };
                push(
                    &mut builder,
                    line,
                    line_num,
                    range.start,
                    range.end,
                    token_type,
                );
            }
        }
    }

    builder.build()
}

/// Push one token for a byte range inside a line, in UTF-16 columns.
fn push(
    builder: &mut SemanticTokensBuilder,
    line: &str,
    line_num: u32,
    start: usize,
    end: usize,
    token_type: TokenType,
) {
    let start_col = utf16_len(&line[..start]);
    let length = utf16_len(&line[start..end]);
    builder.push(line_num, start_col, length, token_type, 0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;

    fn tokens(text: &str) -> Vec<SemanticToken> {
        let idx = LineIndex::new(text);
        generate_semantic_tokens(Source::new(text, &idx))
    }

    #[test]
    fn a_comment_is_one_comment_token() {
        let toks = tokens("# install the man page\n");
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].token_type, TokenType::Comment as u32);
    }

    #[test]
    fn a_plain_path_is_a_value() {
        let toks = tokens("usr/share/man/man8/foo.8\n");
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].token_type, TokenType::Value as u32);
        assert_eq!(toks[0].length, 24);
    }

    #[test]
    fn a_substitution_splits_into_value_field_value() {
        let toks = tokens("usr/lib/${DEB_HOST_MULTIARCH}/foo\n");
        let types: Vec<u32> = toks.iter().map(|t| t.token_type).collect();
        assert_eq!(
            types,
            vec![
                TokenType::Value as u32,
                TokenType::Field as u32,
                TokenType::Value as u32,
            ]
        );
    }

    #[test]
    fn an_empty_line_has_no_tokens() {
        assert!(tokens("\n").is_empty());
    }
}
