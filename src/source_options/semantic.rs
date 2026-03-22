//! Semantic token generation for debian/source/options files.
//!
//! The file format is simple: one option per line, comments start with '#',
//! options can have values separated by '='.

use tower_lsp_server::ls_types::SemanticToken;

use crate::deb822::semantic::{SemanticTokensBuilder, TokenType};

use super::fields::SOURCE_OPTIONS;

/// Check if the given option name is a known dpkg-source option
fn is_known_option(name: &str) -> bool {
    SOURCE_OPTIONS.iter().any(|opt| opt.name == name)
}

/// Generate semantic tokens for a debian/source/options file.
pub fn generate_semantic_tokens(source_text: &str) -> Vec<SemanticToken> {
    let mut builder = SemanticTokensBuilder::new();

    for (line_num, line) in source_text.lines().enumerate() {
        let line_num = line_num as u32;
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        // Comment lines
        if trimmed.starts_with('#') {
            let start_col = line.find('#').unwrap() as u32;
            let length = (line.len() - start_col as usize) as u32;
            builder.push(line_num, start_col, length, TokenType::Comment, 0);
            continue;
        }

        // Option lines: "option-name" or "option-name = value"
        if let Some(eq_pos) = trimmed.find('=') {
            let option_name = trimmed[..eq_pos].trim();
            let value = trimmed[eq_pos + 1..].trim();

            // Find the actual position of the option name in the original line
            let name_start = line.find(option_name).unwrap_or(0) as u32;
            let name_len = option_name.len() as u32;

            let token_type = if is_known_option(option_name) {
                TokenType::Field
            } else {
                TokenType::UnknownField
            };

            builder.push(
                line_num,
                name_start,
                name_len,
                token_type,
                crate::deb822::semantic::token_modifier::DECLARATION,
            );

            // Value token
            if !value.is_empty() {
                // Find position of value in the original line (after '=')
                let eq_in_line = line.find('=').unwrap();
                let after_eq = &line[eq_in_line + 1..];
                let value_offset_in_after = after_eq.len() - after_eq.trim_start().len();
                let value_start = (eq_in_line + 1 + value_offset_in_after) as u32;
                let value_len = value.len() as u32;

                builder.push(line_num, value_start, value_len, TokenType::Value, 0);
            }
        } else {
            // Boolean option (no value)
            let option_name = trimmed;
            let name_start = line.find(option_name).unwrap_or(0) as u32;
            let name_len = option_name.len() as u32;

            let token_type = if is_known_option(option_name) {
                TokenType::Field
            } else {
                TokenType::UnknownField
            };

            builder.push(
                line_num,
                name_start,
                name_len,
                token_type,
                crate::deb822::semantic::token_modifier::DECLARATION,
            );
        }
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comment_line() {
        let text = "# this is a comment\n";
        let tokens = generate_semantic_tokens(text);

        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_type, TokenType::Comment as u32);
        assert_eq!(tokens[0].length, 19);
    }

    #[test]
    fn test_option_with_value() {
        let text = "compression = \"bzip2\"\n";
        let tokens = generate_semantic_tokens(text);

        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
        assert_eq!(tokens[0].length, 11); // "compression"
        assert_eq!(tokens[1].token_type, TokenType::Value as u32);
    }

    #[test]
    fn test_boolean_option() {
        let text = "single-debian-patch\n";
        let tokens = generate_semantic_tokens(text);

        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
        assert_eq!(tokens[0].length, 19); // "single-debian-patch"
    }

    #[test]
    fn test_unknown_option() {
        let text = "unknown-option\n";
        let tokens = generate_semantic_tokens(text);

        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_type, TokenType::UnknownField as u32);
    }

    #[test]
    fn test_empty_text() {
        let tokens = generate_semantic_tokens("");
        assert_eq!(tokens.is_empty(), true);
    }

    #[test]
    fn test_mixed_content() {
        let text = "# comment\ncompression = xz\nsingle-debian-patch\n";
        let tokens = generate_semantic_tokens(text);

        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].token_type, TokenType::Comment as u32);
        assert_eq!(tokens[1].token_type, TokenType::Field as u32); // compression
        assert_eq!(tokens[2].token_type, TokenType::Value as u32); // xz
        assert_eq!(tokens[3].token_type, TokenType::Field as u32); // single-debian-patch
    }

    #[test]
    fn test_empty_lines_skipped() {
        let text = "compression = xz\n\nsingle-debian-patch\n";
        let tokens = generate_semantic_tokens(text);

        assert_eq!(tokens.len(), 3);
    }

    #[test]
    fn test_delta_positions() {
        let text = "compression = xz\nsingle-debian-patch\n";
        let tokens = generate_semantic_tokens(text);

        assert_eq!(tokens[0].delta_line, 0); // compression on line 0
        assert_eq!(tokens[1].delta_line, 0); // xz on same line
        assert_eq!(tokens[2].delta_line, 1); // single-debian-patch on line 1
    }
}
