use rowan::TextSize;
use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use crate::position::Source;
use lintian_overrides::{AstNode, LintianOverrides, OverrideLine, PackageSpec, Parse};

/// The package type keywords offered in a lintian-overrides spec.
const PACKAGE_TYPES: &[&str] = &["source", "binary", "udeb"];

/// Get completion items for a lintian-overrides file.
///
/// Override grammar:
///   `[[<package>][ <archlist>][ <type>]: ]<lintian-tag>[[*]<context>[*]]`
pub fn get_completions(
    parsed: &Parse<LintianOverrides>,
    src: Source<'_>,
    position: Position,
    tags: &[(String, String)],
    packages: &[String],
    architectures: &[String],
) -> Vec<CompletionItem> {
    let current_line = src.text.lines().nth(position.line as usize).unwrap_or("");
    let col = (position.character as usize).min(current_line.len());
    let before_cursor = &current_line[..col];

    let offset = TextSize::from(offset_at(src.text, position) as u32);
    let tree = parsed.tree();

    let Some(line) = tree.lines().find(|l| {
        let r = l.syntax().text_range();
        r.start() <= offset && offset <= r.end()
    }) else {
        return Vec::new();
    };

    if line.is_comment() {
        return Vec::new();
    }

    // On a half-written line like "foo: " the spec node ends up nested in an
    // ERROR node, so line.package_spec() (which only looks at direct children)
    // can't see it. Search descendants so we still pick up the colon.
    let spec = line.syntax().descendants().find_map(PackageSpec::cast);

    // No spec colon: the first token is ambiguous between a package name and a
    // bare tag, so offer both. Past the first token we cannot tell free-form
    // context from a spec-in-progress, so offer nothing.
    if spec.is_none() {
        let (committed, pending) = split_committed(before_cursor);
        if committed.is_empty() {
            let mut out = package_items(packages, pending);
            out.extend(tag_items(pending, tags));
            return out;
        }
        return Vec::new();
    }

    // We have a colon. If the cursor is still inside the spec, complete its
    // arch list or type slot; otherwise it's at or past the colon, so move on
    // to the tag. The "foo: " case lands here too, with the cursor past it.
    match spec {
        Some(spec) if offset < spec.syntax().text_range().end() && !before_cursor.contains(':') => {
            // Inside the spec: architecture list or type slot.
            spec_region_completions(&spec, offset, before_cursor, architectures)
        }
        _ => {
            // At or past the colon: the lintian tag, then free-form context.
            tag_region_completions(&line, offset, before_cursor, tags)
        }
    }
}

/// Completions after the spec colon: the lintian tag, then free-form context.
fn tag_region_completions(
    line: &OverrideLine,
    offset: TextSize,
    before_cursor: &str,
    tags: &[(String, String)],
) -> Vec<CompletionItem> {
    match line.tag() {
        // Cursor is on (or right at the end of) the tag word -> filter tags.
        Some(tag) if offset <= tag.text_range().end() => {
            tag_items(pending_word(before_cursor), tags)
        }
        // Cursor is past the tag, in the context -> nothing to suggest.
        Some(_) => Vec::new(),
        // Colon but no tag yet -> offer the full tag list.
        None => tag_items(pending_word(before_cursor), tags),
    }
}

/// Completions inside a package spec: architecture names within the bracket
/// list, or the opening `[` / type keywords elsewhere.
fn spec_region_completions(
    spec: &PackageSpec,
    offset: TextSize,
    before_cursor: &str,
    architectures: &[String],
) -> Vec<CompletionItem> {
    if spec.arch_list_contains_offset(offset) {
        return arch_items(architectures, pending_arch(before_cursor));
    }

    let pending = pending_word(before_cursor);
    let mut out = Vec::new();
    if pending.is_empty() && bracket_allowed(spec, offset) {
        out.push(punct("[", "Architecture restriction list"));
    }
    out.extend(type_items(pending));
    out
}

/// Whether `[` can be inserted at `offset` without reordering components
/// already committed in `spec`: no arch-list yet, and not past a type
/// keyword that's already there (archlist must precede type).
fn bracket_allowed(spec: &PackageSpec, offset: TextSize) -> bool {
    if spec.has_arch_list() {
        return false;
    }
    match spec.package_type_range() {
        Some(r) => offset <= r.start(),
        None => true,
    }
}

/// Build tag items filtered by `prefix`.
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

/// Build package-name items (source and binary packages from `debian/control`)
/// filtered by `prefix`.
fn package_items(packages: &[String], prefix: &str) -> Vec<CompletionItem> {
    let p = prefix.to_ascii_lowercase();
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

/// Build architecture items filtered by `prefix`.
fn arch_items(architectures: &[String], prefix: &str) -> Vec<CompletionItem> {
    architectures
        .iter()
        .filter(|arch| arch.as_str().starts_with(prefix))
        .map(|arch| CompletionItem {
            label: arch.clone(),
            kind: Some(CompletionItemKind::VALUE),
            ..Default::default()
        })
        .collect()
}

/// Build type-keyword items (`source`, `binary`, `udeb`) filtered by `prefix`.
fn type_items(prefix: &str) -> Vec<CompletionItem> {
    let p = prefix.to_ascii_lowercase();
    PACKAGE_TYPES
        .iter()
        .filter(|t| t.starts_with(&p))
        .map(|t| CompletionItem {
            label: (*t).to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Package type".to_string()),
            ..Default::default()
        })
        .collect()
}

/// Build a punctuation completion item (`[`).
fn punct(label: &str, detail: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(CompletionItemKind::OPERATOR),
        detail: Some(detail.to_string()),
        insert_text: Some(label.to_string()),
        ..Default::default()
    }
}

/// The partial word immediately before the cursor. Used only to filter
/// candidates, never to derive structure.
fn pending_word(before_cursor: &str) -> &str {
    before_cursor
        .rsplit(char::is_whitespace)
        .next()
        .unwrap_or("")
}

/// Like [`pending_word`] but strips the leading `[` / `!` that precede an
/// architecture inside a bracket list.
fn pending_arch(before_cursor: &str) -> &str {
    pending_word(before_cursor)
        .trim_start_matches('[')
        .trim_start_matches('!')
}

/// Split `before_cursor` into the already-committed text and the word currently
/// being typed.
fn split_committed(before_cursor: &str) -> (&str, &str) {
    let trimmed = before_cursor.trim_start();
    match trimmed.rfind(char::is_whitespace) {
        Some(i) => (trimmed[..i].trim_end(), trimmed[i..].trim_start()),
        None => ("", trimmed),
    }
}

/// Byte offset into `text` for an LSP position. Columns are treated as byte
/// offsets (lintian-overrides content is ASCII).
fn offset_at(text: &str, position: Position) -> usize {
    let mut offset = 0;
    for (row, line) in text.split_inclusive('\n').enumerate() {
        if row as u32 == position.line {
            return offset + (position.character as usize).min(line.len());
        }
        offset += line.len();
    }
    offset + position.character as usize
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

    fn pkgs() -> Vec<String> {
        vec!["libcurl4".to_string(), "libfoo-dev".to_string()]
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
        complete_with(line, ch, &[])
    }

    fn complete_with(line: &str, ch: u32, packages: &[String]) -> Vec<CompletionItem> {
        let idx = LineIndex::new(line);
        let src = Source::new(line, &idx);
        let parsed = LintianOverrides::parse(line);
        get_completions(
            &parsed,
            src,
            Position::new(0, ch),
            &tags(),
            packages,
            &archs(),
        )
    }

    fn labels(items: &[CompletionItem]) -> Vec<String> {
        items.iter().map(|c| c.label.clone()).collect()
    }

    #[test]
    fn first_token_offers_packages_and_tags() {
        let items = complete_with("lib", 3, &pkgs());
        let l = labels(&items);
        assert!(l.iter().any(|s| s == "libcurl4"));
        assert!(l.iter().any(|s| s == "libfoo-dev"));
    }

    #[test]
    fn first_token_filters_tags() {
        let items = complete("missing-", 8);
        let l = labels(&items);
        assert!(l.iter().any(|s| s == "missing-systemd-service"));
        assert!(l.iter().any(|s| s == "missing-build-dependency"));
        assert!(!l.iter().any(|s| s == "hardening-no-pie"));
    }

    #[test]
    fn second_token_without_colon_offers_nothing() {
        assert!(complete_with("libcurl4 ", 9, &pkgs()).is_empty());
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
    fn context_after_tag_offers_nothing() {
        assert!(complete("foo: some-tag ", 14).is_empty());
    }

    #[test]
    fn inside_brackets_offers_archs() {
        let items = complete("libcurl4 []: hardening-no-pie", 10);
        let l = labels(&items);
        assert!(l.iter().any(|s| s == "amd64"));
        assert!(l.iter().any(|s| s == "arm64"));
    }

    #[test]
    fn inside_brackets_filters_archs() {
        let items = complete("libcurl4 [arm]: hardening-no-pie", 13);
        let l = labels(&items);
        assert!(l.iter().any(|s| s == "arm64"));
        assert!(l.iter().any(|s| s == "armhf"));
        assert!(!l.iter().any(|s| s == "amd64"));
    }

    #[test]
    fn type_slot_offers_types() {
        let items = complete("foo binary: x", 4);
        let l = labels(&items);
        assert!(l.iter().any(|s| s == "binary"));
        assert!(l.iter().any(|s| s == "source"));
        assert!(l.iter().any(|s| s == "udeb"));
    }

    #[test]
    fn bracket_offered_after_package() {
        let items = complete("foo binary: x", 4);
        let l = labels(&items);
        assert!(l.iter().any(|s| s == "["));
    }

    #[test]
    fn bracket_not_offered_with_existing_arch_list() {
        let items = complete("foo [amd64] binary: x", 12);
        let l = labels(&items);
        assert!(!l.iter().any(|s| s == "["));
        assert!(l.iter().any(|s| s == "binary"));
    }

    #[test]
    fn bracket_not_offered_past_type() {
        let items = complete("foo binary : x", 11);
        let l = labels(&items);
        assert!(!l.iter().any(|s| s == "["));
    }

    #[test]
    fn comment_offers_nothing() {
        assert!(complete("# a comment", 11).is_empty());
    }

    #[test]
    fn after_colon_no_tag_offers_all_tags() {
        let items = complete("foo: ", 5);
        assert_eq!(labels(&items).len(), 3); // toute la liste de tags
    }
}
