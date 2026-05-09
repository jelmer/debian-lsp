use text_size::TextSize;
use tower_lsp_server::ls_types::SemanticToken;

use super::fields::{PACKAGE_KEYS, SOURCE_KEYS, TOP_LEVEL_KEYS};
use crate::position::Source;

/// Token type index for `debianField` (known key).
pub const DEBIAN_FIELD: u32 = 0;
/// Token type index for `debianUnknownField` (unrecognised key).
pub const DEBIAN_UNKNOWN_FIELD: u32 = 1;

#[derive(Debug, PartialEq)]
enum TableContext {
    TopLevel,
    Source,
    Package,
    Unknown,
}

fn find_current_table(lines: &[&str], line_idx: usize) -> TableContext {
    if lines.is_empty() {
        return TableContext::TopLevel;
    }
    let bound = line_idx.min(lines.len() - 1);
    for i in (0..=bound).rev() {
        let line = lines[i].trim();
        if line.starts_with("[packages.") {
            return TableContext::Package;
        }
        if line == "[source]" {
            return TableContext::Source;
        }
        if line.starts_with('[') && !line.starts_with("[[") {
            return TableContext::Unknown;
        }
    }
    TableContext::TopLevel
}

fn is_known_key(key: &str, table: &TableContext) -> bool {
    match table {
        TableContext::TopLevel => TOP_LEVEL_KEYS.iter().any(|k| k.name == key),
        TableContext::Source => SOURCE_KEYS.iter().any(|k| k.name == key),
        TableContext::Package => PACKAGE_KEYS.iter().any(|k| k.name == key),
        TableContext::Unknown => false,
    }
}

/// Generate semantic tokens for a debcargo.toml file.
///
/// Highlights key names as `debianField` (known) or `debianUnknownField`
/// (unrecognised). Values and table headers are left for the editor's
/// generic TOML highlighting.
pub fn generate_semantic_tokens(text: &str, src: Source<'_>) -> Vec<SemanticToken> {
    let lines: Vec<&str> = text.lines().collect();
    let mut tokens: Vec<SemanticToken> = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    // Track the running byte offset of the start of each line.
    let mut line_byte_offset: usize = 0;

    for (line_idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        // Skip comments and table headers.
        if trimmed.starts_with('#') || trimmed.starts_with('[') {
            line_byte_offset += line.len() + 1; // +1 for '\n'
            continue;
        }

        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].trim();
            if !key.is_empty() {
                // The key starts at the first non-space byte within the line.
                let key_col = line.find(key).unwrap_or(0) as u32;
                let key_len = key.len() as u32;

                let table = find_current_table(&lines, line_idx);
                let token_type = if is_known_key(key, &table) {
                    DEBIAN_FIELD
                } else {
                    DEBIAN_UNKNOWN_FIELD
                };

                // Convert the key's byte offset to an LSP line/col.
                let key_byte = line_byte_offset + line.find(key).unwrap_or(0);
                let pos = src.offset_to_position(TextSize::try_from(key_byte).unwrap_or_default());

                let delta_line = pos.line - prev_line;
                let delta_start = if delta_line == 0 {
                    pos.character - prev_start
                } else {
                    pos.character
                };

                tokens.push(SemanticToken {
                    delta_line,
                    delta_start,
                    length: key_len,
                    token_type,
                    token_modifiers_bitset: 0,
                });

                prev_line = pos.line;
                prev_start = key_col;
            }
        }

        line_byte_offset += line.len() + 1;
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;

    fn token_types(text: &str) -> Vec<u32> {
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        generate_semantic_tokens(text, src)
            .into_iter()
            .map(|t| t.token_type)
            .collect()
    }

    #[test]
    fn test_known_top_level_key() {
        let text = "uploaders = []\n";
        assert_eq!(token_types(text), vec![DEBIAN_FIELD]);
    }

    #[test]
    fn test_unknown_top_level_key() {
        let text = "foobar = true\n";
        assert_eq!(token_types(text), vec![DEBIAN_UNKNOWN_FIELD]);
    }

    #[test]
    fn test_source_known_key() {
        let text = "[source]\nsection = \"rust\"\n";
        assert_eq!(token_types(text), vec![DEBIAN_FIELD]);
    }

    #[test]
    fn test_package_known_key() {
        let text = "[packages.lib]\nbreaks = []\n";
        assert_eq!(token_types(text), vec![DEBIAN_FIELD]);
    }

    #[test]
    fn test_table_header_skipped() {
        let text = "[source]\n";
        assert_eq!(token_types(text), Vec::<u32>::new());
    }

    #[test]
    fn test_comment_skipped() {
        let text = "# comment\nuploaders = []\n";
        assert_eq!(token_types(text), vec![DEBIAN_FIELD]);
    }
}
