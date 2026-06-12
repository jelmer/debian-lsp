//! Semantic token generation for debian/conffiles files.

use crate::deb822::semantic::{token_modifier, SemanticTokensBuilder, TokenType};
use crate::position::utf16_len;
use tower_lsp_server::ls_types::SemanticToken;

use super::fields::CONFFILES_FLAGS;

/// Generate semantic tokens for a debian/conffiles file.
pub fn generate_semantic_tokens(source_text: &str) -> Vec<SemanticToken> {
    let mut builder = SemanticTokensBuilder::new();
    let remove_on_upgrade_flag = CONFFILES_FLAGS[0].0;

    for (line_num, line) in source_text.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        // Comment line
        if trimmed.starts_with('#') {
            builder.push(
                line_num as u32,
                0,
                utf16_len(trimmed),
                TokenType::Comment,
                0,
            );
            continue;
        }

        let mut col = 0u32;
        // remove-on-upgrade flag
        if trimmed.starts_with(remove_on_upgrade_flag) {
            builder.push(
                line_num as u32,
                col,
                utf16_len(remove_on_upgrade_flag),
                TokenType::Field,
                0,
            );
            col += utf16_len(remove_on_upgrade_flag) + 1; // remove_on_upgrade_flag + space
        }
        // Absolute path
        let after_flag = trimmed.get(col as usize..).unwrap_or("");
        let spaces = after_flag.len() - after_flag.trim_start().len();
        col += spaces as u32;
        let path = trimmed.get(col as usize..).unwrap_or("").trim_start();
        let path_token = path.split_whitespace().next().unwrap_or("");
        if path_token.starts_with('/') {
            builder.push(
                line_num as u32,
                col,
                utf16_len(path_token),
                TokenType::Value,
                token_modifier::DECLARATION,
            );
        }
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deb822::semantic::TokenType;

    #[test]
    fn test_path_is_value() {
        let tokens = generate_semantic_tokens("/etc/myapp/config.conf\n");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_type, TokenType::Value as u32);
        assert_eq!(tokens[0].length, utf16_len("/etc/myapp/config.conf"));
    }

    #[test]
    fn test_flag_and_path() {
        let tokens = generate_semantic_tokens("remove-on-upgrade /etc/myapp/old.conf\n");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
        assert_eq!(tokens[0].length, utf16_len("remove-on-upgrade"));
        assert_eq!(tokens[1].token_type, TokenType::Value as u32);
        assert_eq!(tokens[1].length, utf16_len("/etc/myapp/old.conf"));
    }

    #[test]
    fn test_empty_line_skipped() {
        let tokens = generate_semantic_tokens("\n\n/etc/foo\n");
        assert_eq!(tokens.len(), 1);
    }

    #[test]
    fn test_multiple_lines() {
        let text = "/etc/myapp/config.conf\nremove-on-upgrade /etc/myapp/old.conf\n";
        let tokens = generate_semantic_tokens(text);
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].token_type, TokenType::Value as u32);
        assert_eq!(tokens[1].token_type, TokenType::Field as u32);
        assert_eq!(tokens[2].token_type, TokenType::Value as u32);
    }

    #[test]
    fn test_path_with_trailing_text_only_highlights_path() {
        let tokens = generate_semantic_tokens("remove-on-upgrade /etc/myapp/extra.conf awe\n");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[1].token_type, TokenType::Value as u32);
        assert_eq!(tokens[1].length, utf16_len("/etc/myapp/extra.conf"));
    }

    #[test]
    fn test_relative_path_not_highlighted() {
        let tokens = generate_semantic_tokens("etc/myapp/config.conf\n");
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_flag_without_path_emits_only_flag() {
        let tokens = generate_semantic_tokens("remove-on-upgrade\n");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_type, TokenType::Field as u32);
    }

    #[test]
    fn test_delta_lines_correct() {
        let text = "/etc/foo\n\n/etc/bar\n";
        let tokens = generate_semantic_tokens(text);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].delta_line, 0);
        assert_eq!(tokens[1].delta_line, 2); // ligne vide entre les deux
    }

    #[test]
    fn test_comment_is_comment() {
        let tokens = generate_semantic_tokens("# this is a comment\n");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_type, TokenType::Comment as u32);
    }
}
