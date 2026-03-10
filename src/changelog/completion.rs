use rowan::ast::AstNode;
use text_size::{TextRange, TextSize};
use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Documentation, Position};

use super::fields::{get_debian_distributions, URGENCY_LEVELS};

#[derive(Debug, Clone, PartialEq, Eq)]
enum CursorContext {
    DistributionValue { value_prefix: String },
    UrgencyValue { value_prefix: String },
}

/// Get completion items for a changelog file at the given cursor position.
///
/// Uses changelog CST context to return only relevant value completions:
/// distributions in header distribution position, urgency levels for
/// `urgency=` metadata values.
pub fn get_completions(
    parse: &debian_changelog::Parse<debian_changelog::ChangeLog>,
    source_text: &str,
    position: Position,
) -> Vec<CompletionItem> {
    // Use syntax_node() + cast so completion keeps working on syntactically
    // invalid/incomplete input while the user is typing.
    let Some(changelog) = debian_changelog::ChangeLog::cast(parse.syntax_node()) else {
        return Vec::new();
    };

    match get_cursor_context(&changelog, source_text, position) {
        Some(CursorContext::DistributionValue { value_prefix }) => {
            get_distribution_completions(&value_prefix)
        }
        Some(CursorContext::UrgencyValue { value_prefix }) => {
            get_urgency_completions(&value_prefix)
        }
        None => Vec::new(),
    }
}

fn get_cursor_context(
    changelog: &debian_changelog::ChangeLog,
    source_text: &str,
    position: Position,
) -> Option<CursorContext> {
    let offset = crate::position::try_position_to_offset(source_text, position)?;
    let entry = changelog.entry_at_offset(offset)?;
    let header = entry.header()?;

    if let Some(value_prefix) = distribution_prefix_at_offset(&header, offset) {
        return Some(CursorContext::DistributionValue { value_prefix });
    }

    if let Some(value_prefix) = urgency_prefix_at_offset(&header, offset) {
        return Some(CursorContext::UrgencyValue { value_prefix });
    }

    None
}

fn distribution_prefix_at_offset(
    header: &debian_changelog::EntryHeader,
    offset: TextSize,
) -> Option<String> {
    let distributions = header
        .syntax()
        .children()
        .find(|n| n.kind() == debian_changelog::SyntaxKind::DISTRIBUTIONS)?;
    let range = distributions.text_range();

    if !range_contains_offset(range, offset) {
        // If distributions are currently empty, still offer completions when the
        // cursor is between the version and semicolon (on whitespaces).
        if range.start() == range.end() && offset < range.start() {
            let version_end = header
                .syntax()
                .children_with_tokens()
                .find_map(|it| {
                    let token = it.as_token()?;
                    if token.kind() == debian_changelog::SyntaxKind::VERSION {
                        Some(token.text_range().end())
                    } else {
                        None
                    }
                });
            if version_end.is_some_and(|end| offset >= end) {
                return Some(String::new());
            }
        }
        return None;
    }

    if range.start() == range.end() {
        return Some(String::new());
    }

    let token = match distributions.token_at_offset(offset) {
        rowan::TokenAtOffset::Single(token) => Some(token),
        rowan::TokenAtOffset::Between(left, right) => {
            if left.kind() == debian_changelog::SyntaxKind::IDENTIFIER {
                Some(left)
            } else {
                Some(right)
            }
        }
        rowan::TokenAtOffset::None => None,
    };

    if let Some(token) = token {
        if token.kind() == debian_changelog::SyntaxKind::IDENTIFIER {
            return Some(token_prefix(token.text(), token.text_range(), offset));
        }
    }

    Some(String::new())
}

fn urgency_prefix_at_offset(
    header: &debian_changelog::EntryHeader,
    offset: TextSize,
) -> Option<String> {
    for metadata in header.metadata_nodes() {
        if !metadata
            .key()
            .is_some_and(|key| key.eq_ignore_ascii_case("urgency"))
        {
            continue;
        }

        let value_node = metadata
            .syntax()
            .children()
            .find(|n| n.kind() == debian_changelog::SyntaxKind::METADATA_VALUE);

        if let Some(value_node) = value_node {
            let value_range = value_node.text_range();
            if !range_contains_offset(value_range, offset) {
                continue;
            }

            if value_range.start() == value_range.end() {
                return Some(String::new());
            }

            let prefix_end = std::cmp::min(offset, value_range.end());
            let token = match value_node.token_at_offset(prefix_end) {
                rowan::TokenAtOffset::Single(token) => Some(token),
                rowan::TokenAtOffset::Between(left, right) => {
                    if left.kind() == debian_changelog::SyntaxKind::IDENTIFIER {
                        Some(left)
                    } else if right.kind() == debian_changelog::SyntaxKind::IDENTIFIER {
                        Some(right)
                    } else {
                        None
                    }
                }
                rowan::TokenAtOffset::None => None,
            };

            if let Some(token) = token {
                if token.kind() == debian_changelog::SyntaxKind::IDENTIFIER {
                    return Some(token_prefix(token.text(), token.text_range(), prefix_end));
                }
            }

            return Some(String::new());
        }

        // In incomplete input like `urgency=`, parser may produce no METADATA_VALUE.
        // Offer urgency completions when cursor is positioned right after '='.
        let equals = metadata
            .syntax()
            .children_with_tokens()
            .filter_map(|it| it.as_token().cloned())
            .find(|token| token.kind() == debian_changelog::SyntaxKind::EQUALS);
        if let Some(equals) = equals {
            let metadata_range = metadata.syntax().text_range();
            if offset >= equals.text_range().end() && offset <= metadata_range.end() {
                return Some(String::new());
            }
        } else {
            continue;
        }
    }

    None
}

fn range_contains_offset(range: TextRange, offset: TextSize) -> bool {
    if range.start() == range.end() {
        offset == range.start()
    } else {
        offset >= range.start() && offset <= range.end()
    }
}

fn token_prefix(token_text: &str, token_range: TextRange, offset: TextSize) -> String {
    let relative_end: usize = (std::cmp::min(offset, token_range.end()) - token_range.start()).into();
    let mut prefix_end = std::cmp::min(relative_end, token_text.len());
    while !token_text.is_char_boundary(prefix_end) {
        prefix_end -= 1;
    }
    token_text[..prefix_end].to_string()
}

/// Get completion items for Debian distributions.
pub fn get_distribution_completions(prefix: &str) -> Vec<CompletionItem> {
    let normalized_prefix = prefix.trim().to_ascii_lowercase();

    get_debian_distributions()
        .into_iter()
        .filter(|dist| dist.to_ascii_lowercase().starts_with(&normalized_prefix))
        .map(|dist| CompletionItem {
            label: dist.clone(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Debian distribution".to_string()),
            documentation: Some(Documentation::String(format!(
                "Target distribution: {}",
                dist
            ))),
            insert_text: Some(dist),
            ..Default::default()
        })
        .collect()
}

/// Get completion items for urgency levels.
pub fn get_urgency_completions(prefix: &str) -> Vec<CompletionItem> {
    let normalized_prefix = prefix.trim().to_ascii_lowercase();

    URGENCY_LEVELS
        .iter()
        .filter(|level| level.name.starts_with(&normalized_prefix))
        .map(|level| CompletionItem {
            label: level.name.to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Urgency level".to_string()),
            documentation: Some(Documentation::String(level.description.to_string())),
            insert_text: Some(level.name.to_string()),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_for(text: &str) -> debian_changelog::Parse<debian_changelog::ChangeLog> {
        debian_changelog::ChangeLog::parse(text)
    }

    fn position_at(text: &str, byte_offset: usize) -> Position {
        crate::position::offset_to_position(text, TextSize::try_from(byte_offset).unwrap())
    }

    #[test]
    fn test_get_completions_on_distribution_value() {
        let text = "foo (1.0-1) un; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = parse_for(text);
        let offset = text.find("un;").unwrap() + 2;
        let completions = get_completions(&parsed, text, position_at(text, offset));

        assert!(!completions.is_empty());
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"unstable"));
        assert!(labels.contains(&"UNRELEASED"));
    }

    #[test]
    fn test_get_completions_on_empty_distribution_slot_with_whitespace() {
        let text = "foo (1.0-1) ; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = parse_for(text);
        let offset = text.find(" ;").unwrap() + 1;
        let completions = get_completions(&parsed, text, position_at(text, offset));

        assert!(!completions.is_empty());
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"unstable"));
    }

    #[test]
    fn test_get_completions_on_urgency_value() {
        let text = "foo (1.0-1) unstable; urgency=me\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = parse_for(text);
        let offset = text.find("urgency=me").unwrap() + "urgency=me".len();
        let completions = get_completions(&parsed, text, position_at(text, offset));

        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["medium"]);
        assert_eq!(completions[0].insert_text.as_deref(), Some("medium"));
    }

    #[test]
    fn test_get_completions_in_body_returns_empty() {
        let text = "foo (1.0-1) unstable; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = parse_for(text);
        let offset = text.find("Initial").unwrap() + 2;
        let completions = get_completions(&parsed, text, position_at(text, offset));
        assert!(completions.is_empty());
    }

    #[test]
    fn test_get_completions_invalid_position_returns_empty() {
        let text = "foo (1.0-1) unstable; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = parse_for(text);

        // Invalid character on an existing line.
        let completions = get_completions(&parsed, text, Position::new(0, 5000));
        assert!(completions.is_empty());

        // Invalid line beyond the end of file.
        let completions = get_completions(&parsed, text, Position::new(5000, 0));
        assert!(completions.is_empty());
    }

    #[test]
    fn test_get_completions_with_parse_errors_still_returns_contextual_results() {
        let text = "foo (1.0-1) unstable; urgency=\n";
        let parsed = parse_for(text);
        assert!(!parsed.ok(), "test setup expects parse errors");

        let offset = text.find("urgency=").unwrap() + "urgency=".len();
        let completions = get_completions(&parsed, text, position_at(text, offset));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"medium"));

        // Invalid line beyond the end of file.
        let completions = get_completions(&parsed, text, Position::new(5000, 0));
        assert!(completions.is_empty());
    }

    #[test]
    fn test_distribution_completions_with_prefix() {
        let completions = get_distribution_completions("un");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"unstable"));
        assert!(labels.contains(&"UNRELEASED"));
    }

    #[test]
    fn test_urgency_completions_with_prefix() {
        let completions = get_urgency_completions("me");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["medium"]);
    }
}
