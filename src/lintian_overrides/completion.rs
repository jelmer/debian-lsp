use rowan::TextSize;
use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, Position};

use crate::position::Source;
use lintian_overrides::{
    AstNode, LintianOverrides, OverrideLine, PackageSpec, Parse, SpecSlot, SyntaxToken,
    PACKAGE_TYPES,
};

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
    let Some(offset) = src.try_position_to_offset(position) else {
        return Vec::new();
    };

    let tree = parsed.tree();

    let line = tree.lines().find(|l| {
        let r = l.syntax().text_range();
        r.start() <= offset && offset <= r.end()
    });

    let Some(line) = line else {
        // Position is past every parsed line, typically because the user just
        // pressed Enter and the cursor is on an unparsed blank line. Treat it
        // like the empty-line case: offer packages and tags.
        let mut out = package_items(packages, "");
        out.extend(tag_items("", tags));
        return out;
    };

    if line.is_comment() {
        return Vec::new();
    }

    match line.package_spec() {
        None => no_spec_completions(&line, offset, packages, tags),
        Some(spec) if offset < spec.syntax().text_range().end() => {
            spec_region_completions(&spec, offset, packages, architectures)
        }
        Some(_) => tag_region_completions(&line, offset, tags),
    }
}

/// A line with no `PACKAGE_SPEC` node: either empty (offer packages and tags)
/// or a bare tag being typed as the first token.
fn no_spec_completions(
    line: &OverrideLine,
    offset: TextSize,
    packages: &[String],
    tags: &[(String, String)],
) -> Vec<CompletionItem> {
    match line.tag() {
        Some(tag) if offset <= tag.text_range().end() => {
            let prefix = token_prefix(&tag, offset);
            let mut out = package_items(packages, prefix);
            out.extend(tag_items(prefix, tags));
            out
        }
        Some(_) => Vec::new(),
        None => {
            let mut out = package_items(packages, "");
            out.extend(tag_items("", tags));
            out
        }
    }
}

/// Completions after the spec colon: the lintian tag, then free-form context.
fn tag_region_completions(
    line: &OverrideLine,
    offset: TextSize,
    tags: &[(String, String)],
) -> Vec<CompletionItem> {
    match line.tag() {
        Some(tag) if offset <= tag.text_range().end() => {
            tag_items(token_prefix(&tag, offset), tags)
        }
        // Cursor is past the tag, in the context -> nothing to suggest.
        Some(_) => Vec::new(),
        // Colon but no tag yet -> offer the full tag list.
        None => tag_items("", tags),
    }
}

/// Completions inside a package spec: routes on which named slot the cursor
/// sits on, per [`PackageSpec::slot_at_offset`].
fn spec_region_completions(
    spec: &PackageSpec,
    offset: TextSize,
    packages: &[String],
    architectures: &[String],
) -> Vec<CompletionItem> {
    // The arch-list region covers empty brackets ("[]") too, where no ARCH
    // token exists; check it first so the user still gets arch completions.
    if spec.arch_list_contains_offset(offset) {
        let prefix = match spec.slot_at_offset(offset) {
            SpecSlot::Arch { text, range } => slot_prefix(&text, range.start(), offset)
                .trim_start_matches('!')
                .to_string(),
            _ => String::new(),
        };
        return arch_items(architectures, &prefix);
    }

    match spec.slot_at_offset(offset) {
        SpecSlot::PackageName { text, range } => {
            package_items(packages, slot_prefix(&text, range.start(), offset))
        }
        // The cursor at the start of a type keyword is ambiguous: it could be
        // "on the type" or "in the gap before it". Treat an empty prefix as
        // the latter so `[` is still offered.
        SpecSlot::PackageType { text, range } => {
            let prefix = slot_prefix(&text, range.start(), offset).to_string();
            if prefix.is_empty() {
                blank_slot_completions(spec, offset)
            } else {
                type_items(&prefix)
            }
        }
        SpecSlot::Arch { .. } => Vec::new(),
        SpecSlot::Blank => blank_slot_completions(spec, offset),
    }
}

/// Completions when the cursor is not on any named spec token: the opening
/// `[` (when position allows) plus the full type-keyword list.
fn blank_slot_completions(spec: &PackageSpec, offset: TextSize) -> Vec<CompletionItem> {
    let mut out = Vec::new();
    if bracket_allowed(spec, offset) {
        out.push(punct("[", "Architecture restriction list"));
    }
    out.extend(type_items(""));
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

/// The token text from `start` up to `offset`, used as the completion filter
/// prefix. `start` must be the token's start offset (from the AST).
fn slot_prefix(text: &str, start: TextSize, offset: TextSize) -> &str {
    let end = usize::from(offset.min(start + TextSize::of(text)) - start);
    &text[..end]
}

/// Like [`slot_prefix`] but for a `SyntaxToken` in-hand.
fn token_prefix(token: &SyntaxToken, offset: TextSize) -> &str {
    slot_prefix(token.text(), token.text_range().start(), offset)
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
    let p = prefix.to_ascii_lowercase();
    architectures
        .iter()
        .filter(|arch| arch.to_ascii_lowercase().starts_with(&p))
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

    fn labels(items: &[CompletionItem]) -> Vec<&str> {
        items.iter().map(|c| c.label.as_str()).collect()
    }

    #[test]
    fn first_token_offers_packages_and_tags() {
        let items = complete_with("lib", 3, &pkgs());
        let l = labels(&items);
        assert!(l.contains(&"libcurl4"));
        assert!(l.contains(&"libfoo-dev"));
    }

    #[test]
    fn first_token_filters_tags() {
        let items = complete("missing-", 8);
        let l = labels(&items);
        assert!(l.contains(&"missing-systemd-service"));
        assert!(l.contains(&"missing-build-dependency"));
        assert!(!l.contains(&"hardening-no-pie"));
    }

    #[test]
    fn second_token_without_colon_offers_nothing() {
        assert!(complete_with("libcurl4 ", 9, &pkgs()).is_empty());
    }

    #[test]
    fn after_colon_offers_tags() {
        let items = complete("foo: ", 5);
        let l = labels(&items);
        assert!(l.contains(&"missing-systemd-service"));
        assert!(l.contains(&"hardening-no-pie"));
    }

    #[test]
    fn after_colon_filters_tags() {
        let items = complete("foo: missing-", 13);
        let l = labels(&items);
        assert!(l.contains(&"missing-systemd-service"));
        assert!(!l.contains(&"hardening-no-pie"));
    }

    #[test]
    fn context_after_tag_offers_nothing() {
        assert!(complete("foo: some-tag ", 14).is_empty());
    }

    #[test]
    fn inside_brackets_offers_archs() {
        let items = complete("libcurl4 []: hardening-no-pie", 10);
        let l = labels(&items);
        assert!(l.contains(&"amd64"));
        assert!(l.contains(&"arm64"));
    }

    #[test]
    fn inside_brackets_filters_archs() {
        let items = complete("libcurl4 [arm]: hardening-no-pie", 13);
        let l = labels(&items);
        assert!(l.contains(&"arm64"));
        assert!(l.contains(&"armhf"));
        assert!(!l.contains(&"amd64"));
    }

    #[test]
    fn inside_brackets_filters_negated_arch() {
        let items = complete("libcurl4 [!am]: hardening-no-pie", 13);
        let l = labels(&items);
        assert!(l.contains(&"amd64"));
        assert!(!l.contains(&"arm64"));
    }

    #[test]
    fn type_slot_offers_types() {
        let items = complete("foo binary: x", 4);
        let l = labels(&items);
        assert!(l.contains(&"binary"));
        assert!(l.contains(&"source"));
        assert!(l.contains(&"udeb"));
    }

    #[test]
    fn bracket_offered_before_type() {
        let items = complete("foo binary: x", 4);
        let l = labels(&items);
        assert!(l.contains(&"["));
    }

    #[test]
    fn bracket_not_offered_with_existing_arch_list() {
        let items = complete("foo [amd64] binary: x", 12);
        let l = labels(&items);
        assert!(!l.contains(&"["));
        assert!(l.contains(&"binary"));
    }

    #[test]
    fn bracket_not_offered_past_type() {
        let items = complete("foo binary : x", 11);
        let l = labels(&items);
        assert!(!l.contains(&"["));
    }

    #[test]
    fn comment_offers_nothing() {
        assert!(complete("# a comment", 11).is_empty());
    }

    #[test]
    fn after_colon_no_tag_offers_all_tags() {
        let items = complete("foo: ", 5);
        assert_eq!(labels(&items).len(), 3); // the full tag list
    }

    fn complete_multiline(
        text: &str,
        line: u32,
        ch: u32,
        packages: &[String],
    ) -> Vec<CompletionItem> {
        let idx = LineIndex::new(text);
        let src = Source::new(text, &idx);
        let parsed = LintianOverrides::parse(text);
        get_completions(
            &parsed,
            src,
            Position::new(line, ch),
            &tags(),
            packages,
            &archs(),
        )
    }

    #[test]
    fn empty_file_offers_completions() {
        let items = complete_with("", 0, &pkgs());
        let l = labels(&items);
        assert!(l.contains(&"libcurl4"));
        assert!(l.contains(&"missing-systemd-service"));
    }

    #[test]
    fn fresh_blank_line_after_override_offers_completions() {
        let items = complete_multiline("foo: some-tag\n", 1, 0, &pkgs());
        let l = labels(&items);
        assert!(l.contains(&"libcurl4"));
        assert!(l.contains(&"missing-systemd-service"));
    }

    #[test]
    fn editing_package_name_offers_packages() {
        // Cursor at the start of a parsed spec: pending word is empty, all
        // packages are offered.
        let items = complete_with("libcurl4: some-tag", 0, &pkgs());
        let l = labels(&items);
        assert!(l.contains(&"libcurl4"));
        assert!(l.contains(&"libfoo-dev"));
    }

    #[test]
    fn editing_package_name_filters_by_prefix() {
        // "libf" filters libcurl4 out.
        let items = complete_with("libf: some-tag", 4, &pkgs());
        let l = labels(&items);
        assert!(l.contains(&"libfoo-dev"));
        assert!(!l.contains(&"libcurl4"));
    }
}
