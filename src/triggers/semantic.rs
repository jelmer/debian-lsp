use tower_lsp_server::ls_types::SemanticToken;

use crate::deb822::semantic::{SemanticTokensBuilder, TokenType};
use crate::position::{utf16_len, Source};
use crate::triggers::completion::DIRECTIVES;
use crate::triggers::parser::parse_line;

/// Semantic tokens for a triggers control file.
pub fn generate_semantic_tokens(src: Source<'_>) -> Vec<SemanticToken> {
    let mut builder = SemanticTokensBuilder::new();

    for (line_num, line) in src.text.lines().enumerate() {
        let line_num = line_num as u32;
        let parsed = parse_line(line);

        for (index, word) in parsed.words.iter().enumerate() {
            let token_type = match index {
                0 if is_directive(&line[word.start..word.end]) => TokenType::Field,
                0 => TokenType::UnknownField,
                _ => TokenType::Value,
            };
            push(
                &mut builder,
                line,
                line_num,
                word.start,
                word.end,
                token_type,
            );
        }

        if let Some(comment) = parsed.comment {
            push(
                &mut builder,
                line,
                line_num,
                comment.start,
                comment.end,
                TokenType::Comment,
            );
        }
    }

    builder.build()
}

/// Whether `word` is one of the directives deb-triggers(5) defines.
fn is_directive(word: &str) -> bool {
    DIRECTIVES.iter().any(|&(name, _)| name == word)
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

    fn types(text: &str) -> Vec<u32> {
        let idx = LineIndex::new(text);
        generate_semantic_tokens(Source::new(text, &idx))
            .iter()
            .map(|t| t.token_type)
            .collect()
    }

    #[test]
    fn a_directive_and_its_trigger() {
        assert_eq!(
            types("interest /usr/share/foo\n"),
            vec![TokenType::Field as u32, TokenType::Value as u32]
        );
    }

    #[test]
    fn an_unknown_directive_stands_out() {
        assert_eq!(
            types("intrest foo\n"),
            vec![TokenType::UnknownField as u32, TokenType::Value as u32]
        );
    }

    #[test]
    fn a_trailing_comment_is_a_comment() {
        assert_eq!(
            types("interest foo # why\n"),
            vec![
                TokenType::Field as u32,
                TokenType::Value as u32,
                TokenType::Comment as u32,
            ]
        );
    }

    #[test]
    fn an_empty_line_has_no_tokens() {
        assert!(types("\n").is_empty());
    }
}
