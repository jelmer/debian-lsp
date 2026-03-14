use std::collections::BTreeSet;

use rowan::ast::AstNode;
use text_size::{TextRange, TextSize};
use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Documentation, Position};

use super::fields::{get_debian_distributions, URGENCY_LEVELS};
use crate::bugs::{BugSummary, SharedBugCache};

#[derive(Debug, Clone, PartialEq, Eq)]
enum CursorContext {
    Package {
        value_prefix: String,
    },
    Distribution {
        value_prefix: String,
    },
    Urgency {
        value_prefix: String,
    },
    BugNumber {
        package_name: Option<String>,
        value_prefix: String,
    },
}

/// Get completion items for a changelog file at the given cursor position.
///
/// Uses changelog CST context to return only relevant value completions:
/// package names at header start, distributions in header distribution
/// position, urgency levels for `urgency=` metadata values, and local
/// changelog bug numbers in `Closes: #...` detail contexts.
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
        Some(CursorContext::Package { value_prefix }) => {
            get_package_completions(&changelog, &value_prefix)
        }
        Some(CursorContext::Distribution { value_prefix }) => {
            get_distribution_completions(&value_prefix)
        }
        Some(CursorContext::Urgency { value_prefix }) => get_urgency_completions(&value_prefix),
        Some(CursorContext::BugNumber { value_prefix, .. }) => {
            get_local_bug_completions(&changelog, &value_prefix)
        }
        None => Vec::new(),
    }
}

/// Get bug-number completions for changelog `Closes: #...` context using
/// both local changelog data and cached Debbugs lookups.
///
/// Returns `None` when the cursor is not in bug-number context.
pub async fn get_async_bug_completions(
    parse: &debian_changelog::Parse<debian_changelog::ChangeLog>,
    source_text: &str,
    position: Position,
    bug_cache: &SharedBugCache,
) -> Option<Vec<CompletionItem>> {
    // Keep all CST-backed values in a short scope and drop them before await
    // so this future remains Send for tower-lsp.
    let (package_name, value_prefix, local) = {
        let changelog = debian_changelog::ChangeLog::cast(parse.syntax_node())?;

        let CursorContext::BugNumber {
            package_name,
            value_prefix,
        } = get_cursor_context(&changelog, source_text, position)?
        else {
            return None;
        };

        let local = get_local_bug_completions(&changelog, &value_prefix);
        (package_name, value_prefix, local)
    };

    let mut remote_completions = Vec::new();
    if let Some(package_name) = package_name {
        let mut summaries = bug_cache
            .write()
            .await
            .get_bug_summaries_with_prefix(&package_name, &value_prefix)
            .await;

        // Sort: open bugs first, then by bug ID descending (highest/newest first).
        summaries.sort_by(|a, b| a.done.cmp(&b.done).then(b.id.cmp(&a.id)));

        let normalized_prefix = value_prefix.trim();
        remote_completions = summaries
            .into_iter()
            .enumerate()
            .map(|(idx, summary)| {
                let id_str = summary.id.to_string();
                let detail_text = match &summary.title {
                    Some(title) => format!("Debian bug (from UDD): {}", title),
                    None => "Debian bug (from UDD)".to_string(),
                };
                CompletionItem {
                    label: format!("#{}", id_str),
                    kind: Some(CompletionItemKind::REFERENCE),
                    detail: Some(detail_text),
                    documentation: Some(Documentation::String(bug_summary_documentation(&summary))),
                    sort_text: Some(format!("{:06}", idx)),
                    insert_text: Some(
                        id_str
                            .strip_prefix(normalized_prefix)
                            .unwrap_or(&id_str)
                            .to_string(),
                    ),
                    ..Default::default()
                }
            })
            .collect();
    }

    Some(merge_unique_completions(remote_completions, local))
}

fn get_cursor_context(
    changelog: &debian_changelog::ChangeLog,
    source_text: &str,
    position: Position,
) -> Option<CursorContext> {
    let offset = crate::position::try_position_to_offset(source_text, position)?;
    let entry = changelog.entry_at_offset(offset)?;

    if let Some(header) = entry.header() {
        if let Some(value_prefix) = package_prefix_at_offset(&header, offset) {
            return Some(CursorContext::Package { value_prefix });
        }

        if let Some(value_prefix) = distribution_prefix_at_offset(&header, offset) {
            return Some(CursorContext::Distribution { value_prefix });
        }

        if let Some(value_prefix) = urgency_prefix_at_offset(&header, offset) {
            return Some(CursorContext::Urgency { value_prefix });
        }
    }

    if let Some(value_prefix) = bug_prefix_at_offset(&entry, offset) {
        return Some(CursorContext::BugNumber {
            package_name: entry.package(),
            value_prefix,
        });
    }

    None
}

fn package_prefix_at_offset(
    header: &debian_changelog::EntryHeader,
    offset: TextSize,
) -> Option<String> {
    let header_range = header.syntax().text_range();
    let version_start = header.syntax().children_with_tokens().find_map(|it| {
        let token = it.as_token()?;
        if token.kind() == debian_changelog::SyntaxKind::VERSION {
            Some(token.text_range().start())
        } else {
            None
        }
    });
    let package_slot_end = version_start.unwrap_or_else(|| header_range.end());

    // Package appears at the start of the header, before VERSION.
    if header_range.start() == package_slot_end {
        return if offset == header_range.start() {
            Some(String::new())
        } else {
            None
        };
    }

    if offset < header_range.start() || offset > package_slot_end {
        return None;
    }

    if version_start.is_some_and(|start| offset >= start) {
        return None;
    }

    let package_token = match header.syntax().token_at_offset(offset) {
        rowan::TokenAtOffset::Single(token) => Some(token),
        rowan::TokenAtOffset::Between(left, right) => {
            if left.kind() == debian_changelog::SyntaxKind::IDENTIFIER
                && left.text_range().end() <= package_slot_end
            {
                Some(left)
            } else if right.kind() == debian_changelog::SyntaxKind::IDENTIFIER
                && right.text_range().end() <= package_slot_end
            {
                Some(right)
            } else {
                None
            }
        }
        rowan::TokenAtOffset::None => None,
    };

    if let Some(token) = package_token {
        if token.kind() != debian_changelog::SyntaxKind::IDENTIFIER
            || token.text_range().end() > package_slot_end
        {
            return Some(String::new());
        }

        if offset <= token.text_range().start() {
            return Some(String::new());
        }

        let prefix_end = std::cmp::min(offset, token.text_range().end());
        return Some(token_prefix(token.text(), token.text_range(), prefix_end));
    }

    Some(String::new())
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
            let version_end = header.syntax().children_with_tokens().find_map(|it| {
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

/// Return the decimal prefix after `Closes: #` at `offset` inside an entry.
///
/// This only matches detail lines (`DETAIL` tokens) and returns `None` for
/// non-`Closes:` contexts.
fn bug_prefix_at_offset(entry: &debian_changelog::Entry, offset: TextSize) -> Option<String> {
    let detail = match entry.syntax().token_at_offset(offset) {
        rowan::TokenAtOffset::Single(token) => Some(token),
        rowan::TokenAtOffset::Between(left, right) => {
            if left.kind() == debian_changelog::SyntaxKind::DETAIL {
                Some(left)
            } else if right.kind() == debian_changelog::SyntaxKind::DETAIL {
                Some(right)
            } else {
                None
            }
        }
        rowan::TokenAtOffset::None => None,
    }?;

    if detail.kind() != debian_changelog::SyntaxKind::DETAIL {
        return None;
    }

    closes_bug_prefix_at_offset(detail.text(), detail.text_range(), offset)
}

/// Parse a `Closes:` bug-number prefix from a single detail line slice.
///
/// Supports comma-separated references like `Closes: #123, #456` and returns
/// the digits of the currently edited fragment.
fn closes_bug_prefix_at_offset(
    detail_text: &str,
    detail_range: TextRange,
    offset: TextSize,
) -> Option<String> {
    if !range_contains_offset(detail_range, offset) {
        return None;
    }

    let relative_end: usize =
        (std::cmp::min(offset, detail_range.end()) - detail_range.start()).into();
    let mut prefix_end = std::cmp::min(relative_end, detail_text.len());
    while !detail_text.is_char_boundary(prefix_end) {
        prefix_end -= 1;
    }
    let up_to_cursor = &detail_text[..prefix_end];

    let lower = up_to_cursor.to_ascii_lowercase();
    let closes_pos = lower.rfind("closes:")?;
    let after_closes = &up_to_cursor[closes_pos + "closes:".len()..];

    if after_closes
        .chars()
        .any(|c| !(c.is_ascii_whitespace() || c == ',' || c == '#' || c.is_ascii_digit()))
    {
        return None;
    }

    let current_fragment = after_closes
        .rsplit(',')
        .next()
        .unwrap_or(after_closes)
        .trim_start();
    let digits = current_fragment.strip_prefix('#')?;

    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    Some(digits.to_string())
}

fn range_contains_offset(range: TextRange, offset: TextSize) -> bool {
    if range.start() == range.end() {
        offset == range.start()
    } else {
        offset >= range.start() && offset <= range.end()
    }
}

fn token_prefix(token_text: &str, token_range: TextRange, offset: TextSize) -> String {
    let relative_end: usize =
        (std::cmp::min(offset, token_range.end()) - token_range.start()).into();
    let mut prefix_end = std::cmp::min(relative_end, token_text.len());
    while !token_text.is_char_boundary(prefix_end) {
        prefix_end -= 1;
    }
    token_text[..prefix_end].to_string()
}

/// Get completion items for package names from existing changelog entries.
fn get_package_completions(
    changelog: &debian_changelog::ChangeLog,
    prefix: &str,
) -> Vec<CompletionItem> {
    let normalized_prefix = prefix.trim().to_ascii_lowercase();
    let mut package_names = BTreeSet::new();

    for entry in changelog.iter() {
        if let Some(package_name) = entry.package() {
            package_names.insert(package_name.to_string());
        }
    }

    package_names
        .into_iter()
        .filter(|name| name.to_ascii_lowercase().starts_with(&normalized_prefix))
        .map(|name| CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some("Debian source package".to_string()),
            documentation: Some(Documentation::String(format!(
                "Debian source package: {}",
                name
            ))),
            insert_text: Some(name),
            ..Default::default()
        })
        .collect()
}

fn get_local_bug_completions(
    changelog: &debian_changelog::ChangeLog,
    prefix: &str,
) -> Vec<CompletionItem> {
    let mut bug_ids = BTreeSet::new();
    for entry in changelog.iter() {
        let lines: Vec<_> = entry.change_lines().collect();
        let line_refs: Vec<_> = lines.iter().map(|line| line.as_str()).collect();
        bug_ids.extend(debian_changelog::changes::find_closed_debian_bugs(
            &line_refs,
        ));
    }

    let normalized_prefix = prefix.trim();
    bug_ids
        .into_iter()
        .map(|id| id.to_string())
        .filter(|id| id.starts_with(normalized_prefix))
        .map(|id| CompletionItem {
            label: format!("#{}", id),
            kind: Some(CompletionItemKind::REFERENCE),
            detail: Some("Debian bug (from changelog history)".to_string()),
            insert_text: Some(
                id.strip_prefix(normalized_prefix)
                    .unwrap_or(&id)
                    .to_string(),
            ),
            ..Default::default()
        })
        .collect()
}

fn merge_unique_completions(
    first: Vec<CompletionItem>,
    second: Vec<CompletionItem>,
) -> Vec<CompletionItem> {
    let mut seen = BTreeSet::new();
    first
        .into_iter()
        .chain(second)
        .filter(|item| seen.insert(item.label.clone()))
        .collect()
}

fn bug_summary_documentation(summary: &BugSummary) -> String {
    let mut parts = Vec::new();
    parts.push(format!("https://bugs.debian.org/{}", summary.id));
    if let Some(severity) = &summary.severity {
        parts.push(format!("Severity: {}", severity));
    }
    if summary.done {
        parts.push("Status: done".to_string());
    }
    if let Some(originator) = &summary.originator {
        if !originator.is_empty() {
            parts.push(format!("Reported by: {}", originator));
        }
    }
    if let Some(tags) = &summary.tags {
        if !tags.is_empty() {
            parts.push(format!("Tags: {}", tags));
        }
    }
    if let Some(forwarded) = &summary.forwarded {
        if !forwarded.is_empty() {
            parts.push(format!("Forwarded: {}", forwarded));
        }
    }
    parts.join("\n")
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
    fn test_get_completions_on_package_value() {
        let text = "foo (1.0-1) unstable; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = parse_for(text);
        let offset = text.find("foo (").unwrap() + 2;
        let completions = get_completions(&parsed, text, position_at(text, offset));

        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["foo"]);
        assert_eq!(
            completions[0].detail.as_deref(),
            Some("Debian source package")
        );
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
    fn test_get_completions_on_closes_bug_value() {
        let text = "\
foo (1.0-2) unstable; urgency=medium

  * Follow-up release.

 -- John Doe <john@example.com>  Mon, 02 Jan 2024 12:00:00 +0000

foo (1.0-1) unstable; urgency=medium

  * Fix issue. Closes: #123456

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
";
        let parsed = parse_for(text);
        let offset = text.find("Closes: #12").unwrap() + "Closes: #12".len();
        let completions = get_completions(&parsed, text, position_at(text, offset));
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"#123456"));
    }

    #[test]
    fn test_get_completions_on_non_closes_bug_context_returns_empty() {
        let text = "foo (1.0-1) unstable; urgency=medium\n\n  * Ref #123456 without closes tag.\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = parse_for(text);
        let offset = text.find("#123456").unwrap() + 4;
        let completions = get_completions(&parsed, text, position_at(text, offset));
        assert!(completions.is_empty());
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
    fn test_package_completions_with_prefix() {
        let text = "\
foo (2.0-1) unstable; urgency=medium

  * New release.

 -- John Doe <john@example.com>  Mon, 01 Jan 2025 12:00:00 +0000

bar (1.0-1) unstable; urgency=low

  * Initial release.

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
";
        let parsed = parse_for(text);
        let changelog = parsed.tree();
        let completions = get_package_completions(&changelog, "fo");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["foo"]);
    }

    #[test]
    fn test_urgency_completions_with_prefix() {
        let completions = get_urgency_completions("me");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["medium"]);
    }

    #[test]
    fn test_closes_bug_prefix_detection() {
        let text = "  * Fix issue. Closes: #1234, #5678";
        let offset = TextSize::try_from(text.find("#567").unwrap() + 4).unwrap();
        let prefix = closes_bug_prefix_at_offset(
            text,
            TextRange::new(
                TextSize::from(0u32),
                TextSize::try_from(text.len()).unwrap(),
            ),
            offset,
        );
        assert_eq!(prefix.as_deref(), Some("567"));
    }

    #[test]
    fn test_closes_bug_prefix_rejects_non_closes_context() {
        let text = "  * Mention #123456";
        let offset = TextSize::try_from(text.find("#123").unwrap() + 4).unwrap();
        let prefix = closes_bug_prefix_at_offset(
            text,
            TextRange::new(
                TextSize::from(0u32),
                TextSize::try_from(text.len()).unwrap(),
            ),
            offset,
        );
        assert!(prefix.is_none());
    }

    #[tokio::test]
    async fn test_get_async_bug_completions_merges_local_and_remote() {
        let text = "\
foo (1.0-2) unstable; urgency=medium

  * Follow-up work. Closes: #12

 -- John Doe <john@example.com>  Mon, 02 Jan 2024 12:00:00 +0000

foo (1.0-1) unstable; urgency=medium

  * Older fix. Closes: #123456

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
";
        let parsed = parse_for(text);
        let bug_cache = crate::bugs::new_shared_bug_cache();

        {
            let mut cache = bug_cache.write().await;
            cache.insert_cached_open_bugs_for_package(
                "foo",
                vec![
                    (123456, Some("Older fix from BTS")),
                    (129999, Some("New regression in foo")),
                ],
            );
        }

        let offset = text.find("Closes: #12").unwrap() + "Closes: #12".len();
        let completions =
            get_async_bug_completions(&parsed, text, position_at(text, offset), &bug_cache)
                .await
                .expect("bug context should return Some");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();

        assert!(labels.contains(&"#123456"));
        assert!(labels.contains(&"#129999"));
        let count_123456 = labels.iter().filter(|label| **label == "#123456").count();
        assert_eq!(count_123456, 1);
        assert!(completions.iter().any(|item| {
            item.label == "#129999"
                && item
                    .detail
                    .as_deref()
                    .is_some_and(|detail| detail.contains("New regression in foo"))
        }));
        assert!(completions.iter().any(|item| {
            item.label == "#123456"
                && item
                    .detail
                    .as_deref()
                    .is_some_and(|detail| detail.contains("Older fix from BTS"))
        }));
    }

    #[tokio::test]
    async fn test_get_async_bug_completions_local_only_no_cache() {
        let text = "\
foo (1.0-2) unstable; urgency=medium

  * Fix regression. Closes: #

 -- John Doe <john@example.com>  Mon, 02 Jan 2024 12:00:00 +0000

foo (1.0-1) unstable; urgency=medium

  * Initial fix. Closes: #654321

 -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000
";
        let parsed = parse_for(text);
        let bug_cache = crate::bugs::new_shared_bug_cache();
        let offset = text.find("Closes: #\n").unwrap() + "Closes: #".len();
        let completions =
            get_async_bug_completions(&parsed, text, position_at(text, offset), &bug_cache)
                .await
                .expect("bug context should return Some");
        let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
        assert!(
            labels.contains(&"#654321"),
            "expected #654321, got {:?}",
            labels
        );
    }

    #[tokio::test]
    async fn test_get_async_bug_completions_returns_none_outside_bug_context() {
        let text = "foo (1.0-1) unstable; urgency=medium\n\n  * Initial release.\n\n -- John Doe <john@example.com>  Mon, 01 Jan 2024 12:00:00 +0000\n";
        let parsed = parse_for(text);
        let bug_cache = crate::bugs::new_shared_bug_cache();
        let offset = text.find("Initial").unwrap() + 2;

        let completions =
            get_async_bug_completions(&parsed, text, position_at(text, offset), &bug_cache).await;
        assert!(completions.is_none());
    }
}
