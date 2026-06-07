use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use crate::position::Source;
use lintian_overrides::{parse_package_spec, LintianOverrides, Parse, ParsedSpec, PACKAGE_TYPES};

/// Get completion items for a lintian-overrides file.
///
/// Override grammar:
///   `[[<package>][ <archlist>][ <type>]: ]<lintian-tag>[[*]<context>[*]]`
pub fn get_completions(
    _parsed: &Parse<LintianOverrides>,
    src: Source<'_>,
    position: Position,
    tags: &[(String, String)],
    packages: &[String],
    architectures: &[String],
) -> Vec<CompletionItem> {
    let current_line = src.text.lines().nth(position.line as usize).unwrap_or("");

    if current_line.trim_start().starts_with('#') {
        return Vec::new();
    }

    let before_cursor = &current_line[..position.character as usize];

    match split_on_spec_colon(before_cursor) {
        Some(after_colon) => tag_completions(after_colon, tags),
        None => no_colon_completions(before_cursor, tags, packages, architectures),
    }
}

/// If `before_cursor` contains the spec-terminating colon, return the slice.
fn split_on_spec_colon(before_cursor: &str) -> Option<&str> {
    let mut search_from = 0;
    while let Some(rel) = before_cursor[search_from..].find(':') {
        let pos = search_from + rel;
        let after = &before_cursor[pos + 1..];
        if after.is_empty() || after.starts_with(char::is_whitespace) {
            if parse_package_spec(&before_cursor[..pos]).is_some() {
                return Some(after.trim_start());
            }
        }
        search_from = pos + 1;
    }
    None
}

/// Completions when no spec colon has been typed.
fn no_colon_completions(
    before_cursor: &str,
    tags: &[(String, String)],
    packages: &[String],
    architectures: &[String],
) -> Vec<CompletionItem> {
    let trimmed = before_cursor.trim_start();

    // Inside an unclosed bracket group is the architecture list.
    let in_brackets = match (trimmed.rfind('['), trimmed.rfind(']')) {
        (Some(o), Some(c)) => o > c,
        (Some(_), None) => true,
        _ => false,
    };
    if in_brackets {
        return arch_completions(trimmed, architectures);
    }

    // Split committed components from the token currently being typed.
    let ends_with_space = before_cursor.ends_with(char::is_whitespace);
    let (committed, pending) = if ends_with_space {
        (trimmed, "")
    } else {
        match trimmed.rfind(char::is_whitespace) {
            Some(i) => (trimmed[..i].trim_end(), trimmed[i..].trim_start()),
            None => ("", trimmed),
        }
    };

    // No committed word yet -> typing the first token.
    // Ambiguous: package name or bare tag. Offer both with package name first.
    if committed.is_empty() {
        let mut out = package_completions(packages, pending);
        out.extend(tag_items(pending, tags));
        push_bracket(&mut out, pending);
        push_types(&mut out, pending);
        return out;
    }

    // A word is already committed without a ':'. Classify it.
    match parse_package_spec(committed) {
        Some(spec) if !spec.is_empty() => spec_continuation(&spec, pending),
        // Committed word was a bare tag -> we're in its context. Nothing.
        _ => Vec::new(),
    }
}

/// Propose completions for the next component of a partially-typed spec.
fn spec_continuation(spec: &ParsedSpec, pending: &str) -> Vec<CompletionItem> {
    let mut out = Vec::new();
    if spec.package_type.is_some() {
        // Type keyword committed -> only `:` remains.
        push_colon(&mut out, pending);
    } else if spec.has_arch_list {
        // Arch list committed -> type or `:`.
        push_types(&mut out, pending);
        push_colon(&mut out, pending);
    } else if spec.package.is_some() {
        // Package name committed -> `[`, type, or `:`.
        push_bracket(&mut out, pending);
        push_types(&mut out, pending);
        push_colon(&mut out, pending);
    }
    out
}

/// Return tag completion items for the part after the spec colon.
fn tag_completions(after_colon: &str, tags: &[(String, String)]) -> Vec<CompletionItem> {
    let tokens: Vec<&str> = after_colon.split_whitespace().collect();
    if tokens.len() >= 2 || (tokens.len() == 1 && after_colon.ends_with(char::is_whitespace)) {
        return Vec::new();
    }
    tag_items(tokens.first().copied().unwrap_or(""), tags)
}

/// Filter tags by `prefix` and build completion items.
fn tag_items(prefix: &str, tags: &[(String, String)]) -> Vec<CompletionItem> {
    let p = prefix.to_ascii_lowercase();
    tags.iter()
        .filter(|(tag, _)| tag.to_ascii_lowercase().starts_with(&p))
        .map(|(tag, description)| CompletionItem {
            label: tag.clone(),
            kind: Some(CompletionItemKind::VALUE),
            detail: (!description.is_empty()).then(|| description.clone()),
            ..Default::default()
        })
        .collect()
}

/// Architecture completions while inside an unclosed `[ ... ]`.
fn arch_completions(spec: &str, architectures: &[String]) -> Vec<CompletionItem> {
    let after_open = spec.rsplit('[').next().unwrap_or("");
    let pending = after_open
        .rsplit(char::is_whitespace)
        .next()
        .unwrap_or("")
        .trim_start_matches('!');

    let mut out: Vec<CompletionItem> = architectures
        .iter()
        .filter(|arch| arch.as_str().starts_with(pending))
        .map(|arch| CompletionItem {
            label: arch.clone(),
            kind: Some(CompletionItemKind::VALUE),
            ..Default::default()
        })
        .collect();

    if after_open.is_empty() || after_open.ends_with(char::is_whitespace) {
        out.push(punct("]", "Close architecture list"));
    }
    out
}

/// Return completion items for package names (source and binary) from
/// `debian/control`, filtered by `pending`.
fn package_completions(packages: &[String], pending: &str) -> Vec<CompletionItem> {
    let p = pending.to_ascii_lowercase();
    packages
        .iter()
        .filter(|name| name.to_ascii_lowercase().starts_with(&p))
        .map(|name| CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some("Package".to_string()),
            ..Default::default()
        })
        .collect()
}

/// Push `[` if no token is being typed yet (pending is empty).
fn push_bracket(out: &mut Vec<CompletionItem>, pending: &str) {
    if pending.is_empty() {
        out.push(punct("[", "Architecture restriction list"));
    }
}

/// Push `:` if no token is being typed yet (pending is empty).
fn push_colon(out: &mut Vec<CompletionItem>, pending: &str) {
    if pending.is_empty() {
        out.push(punct(":", "End of package specification"));
    }
}

/// Push matching type keywords (`source`, `binary`, `udeb`) filtered by prefix.
fn push_types(out: &mut Vec<CompletionItem>, pending: &str) {
    let p = pending.to_ascii_lowercase();
    for &t in PACKAGE_TYPES {
        if t.starts_with(&p) {
            out.push(CompletionItem {
                label: t.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("Package type".to_string()),
                ..Default::default()
            });
        }
    }
}

/// Build a punctuation completion item (`[`, `]`, `:`).
fn punct(label: &str, detail: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(CompletionItemKind::OPERATOR),
        detail: Some(detail.to_string()),
        insert_text: Some(label.to_string()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::LineIndex;

    fn tags() -> Vec<(String, String)> {
        vec![
            ("missing-systemd-service".to_string(), "d".to_string()),
            ("missing-build-dependency".to_string(), "d".to_string()),
            ("hardening-no-pie".to_string(), "d".to_string()),
        ]
    }

    fn archs() -> Vec<String> {
        vec![
            "amd64".to_string(),
            "arm64".to_string(),
            "armhf".to_string(),
            "i386".to_string(),
            "any".to_string(),
            "all".to_string(),
            "linux-any".to_string(),
        ]
    }

    fn complete(line: &str, ch: u32) -> Vec<CompletionItem> {
        let idx = LineIndex::new(line);
        let src = Source::new(line, &idx);
        let parsed = LintianOverrides::parse(line);
        get_completions(&parsed, src, Position::new(0, ch), &tags(), &[], &archs())
    }

    fn labels(items: &[CompletionItem]) -> Vec<String> {
        items.iter().map(|c| c.label.clone()).collect()
    }

    #[test]
    fn empty_line_offers_tags_no_control() {
        let items = complete("missing-", 8);
        let l = labels(&items);
        assert!(l.iter().any(|s| s == "missing-systemd-service"));
        assert!(l.iter().any(|s| s == "missing-build-dependency"));
        assert!(!l.iter().any(|s| s == "hardening-no-pie"));
    }

    #[test]
    fn after_package_offers_bracket_type_colon() {
        let items = complete("foo ", 4);
        let l = labels(&items);
        assert!(l.iter().any(|s| s == "["));
        assert!(l.iter().any(|s| s == ":"));
        assert!(l.iter().any(|s| s == "binary"));
    }

    #[test]
    fn lone_type_word_offers_only_colon() {
        let items = complete("source ", 7);
        let l = labels(&items);
        assert_eq!(l, vec![":"]);
    }

    #[test]
    fn inside_bracket_offers_archs_and_close() {
        let items = complete("foo [", 5);
        let l = labels(&items);
        assert!(l.iter().any(|s| s == "amd64"));
        assert!(l.iter().any(|s| s == "]"));
    }

    #[test]
    fn inside_bracket_filters_archs_no_close_mid_token() {
        let items = complete("foo [arm", 8);
        let l = labels(&items);
        assert!(l.iter().any(|s| s == "arm64"));
        assert!(l.iter().any(|s| s == "armhf"));
        assert!(!l.iter().any(|s| s == "amd64"));
        assert!(!l.iter().any(|s| s == "]"));
    }

    #[test]
    fn after_closed_bracket_offers_type_or_colon() {
        let items = complete("foo [amd64] ", 12);
        let l = labels(&items);
        assert!(l.iter().any(|s| s == "binary"));
        assert!(l.iter().any(|s| s == ":"));
        assert!(!l.iter().any(|s| s == "["));
    }

    #[test]
    fn name_plus_type_offers_only_colon() {
        let items = complete("libcurl4 source ", 16);
        let l = labels(&items);
        assert_eq!(l, vec![":"]);
    }

    #[test]
    fn after_colon_offers_tags() {
        let items = complete("foo: ", 5);
        let l = labels(&items);
        assert!(l.iter().any(|s| s == "missing-systemd-service"));
        assert!(l.iter().any(|s| s == "hardening-no-pie"));
    }

    #[test]
    fn after_colon_filters_tags() {
        let items = complete("foo: missing-", 13);
        let l = labels(&items);
        assert!(l.iter().any(|s| s == "missing-systemd-service"));
        assert!(!l.iter().any(|s| s == "hardening-no-pie"));
    }

    #[test]
    fn context_after_spec_tag_offers_nothing() {
        assert!(complete("foo: some-tag ", 14).is_empty());
    }

    #[test]
    fn comment_offers_nothing() {
        assert!(complete("# a comment", 11).is_empty());
    }
}
