//! Detect mentions of `debian/` files in changelog detail lines.
//!
//! Changelog entries routinely refer to other packaging files, e.g.
//! `d/control`, `debian/gbp.conf` or `d/patches/03_fix.patch`. This module
//! locates those mentions so they can be turned into clickable links (LSP
//! document links) or cross-references (SCIP occurrences).
//!
//! Two forms are recognised:
//!
//! - explicit paths (`iter_file_refs`): `d/...` or `debian/...`;
//! - the prose forms `patch <name>` and `patches <a>, <b> and <c>`
//!   (`iter_patch_word_refs`), which name one or more patches in
//!   `debian/patches/` without the directory prefix. This form is only
//!   meaningful when the named patches actually exist, so callers should
//!   validate the resulting path before linking.

/// A file mention found in a changelog detail line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRef {
    /// Byte offset of the mention within the searched text.
    pub start: usize,
    /// Byte offset one past the end of the mention.
    pub end: usize,
    /// The path relative to the source-tree root, always starting with
    /// `debian/` (a leading `d/` is normalised to `debian/`).
    pub path: String,
}

/// Characters that may appear in a path component after the `debian/` prefix.
///
/// Debian file and directory names are conservative; this set covers the
/// realistic cases (alphanumerics, `.`, `_`, `-`, `+`, `/`) without dragging
/// in surrounding punctuation like the trailing dot in "d/tests/control.".
fn is_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+' | '/')
}

/// Iterate file mentions within `text`.
///
/// Recognises substrings beginning with `d/` or `debian/` that are not part
/// of a larger word (the preceding character, if any, must not be a path
/// character). Trailing path-separator and dot characters are trimmed so a
/// sentence-ending `d/control.` yields `debian/control`.
pub fn iter_file_refs(text: &str) -> Vec<FileRef> {
    let bytes = text.as_bytes();
    let mut refs = Vec::new();
    let mut search_from = 0;

    while search_from < text.len() {
        let Some((prefix_off, prefix_len)) = find_prefix(&text[search_from..]) else {
            break;
        };
        let start = search_from + prefix_off;

        // Reject when glued to a preceding path character, so we don't match
        // the `d/` inside e.g. `upstream-d/foo` or a URL like `http://...d/x`.
        if start > 0 {
            let prev = text[..start].chars().next_back().unwrap();
            if is_path_char(prev) {
                search_from = start + prefix_len;
                continue;
            }
        }

        // Consume the path body following the prefix.
        let mut end = start + prefix_len;
        while end < bytes.len() && is_path_char(text[end..].chars().next().unwrap()) {
            end += text[end..].chars().next().unwrap().len_utf8();
        }
        // Trim trailing separators and dots (sentence punctuation, stray slash).
        while end > start + prefix_len && matches!(bytes[end - 1], b'.' | b'/') {
            end -= 1;
        }

        let raw = &text[start..end];
        // Require at least one component after the prefix; bare `d/` or
        // `debian/` is not a file mention.
        if let Some(path) = normalise(raw) {
            refs.push(FileRef { start, end, path });
        }
        search_from = end.max(start + prefix_len);
    }

    refs
}

/// Iterate `patch <name>` and `patches <a>, <b> and <c>` candidates within
/// `text`.
///
/// Matches the word `patch` or `patches` (case-insensitive, not part of a
/// larger word) followed by one or more whitespace-delimited tokens. The
/// plural form accepts comma-separated lists, `and`-joined pairs, and the
/// usual `a, b and c` / `a, b, and c` mixtures. Each token yields a
/// [`FileRef`] whose `path` is `debian/patches/<token>` and whose span covers
/// just that token; trailing sentence punctuation on each token is trimmed.
///
/// This is a loose heuristic (`patch to fix the build` would match `to`), so
/// callers must check that the path exists before turning it into a link.
pub fn iter_patch_word_refs(text: &str) -> Vec<FileRef> {
    let mut refs = Vec::new();
    let mut search_from = 0;

    while let Some((rel, plural)) = find_patch_word(&text[search_from..]) {
        let word_start = search_from + rel;
        let word_len = if plural {
            "patches".len()
        } else {
            "patch".len()
        };
        let after = word_start + word_len;
        search_from = after;

        // Require whitespace separating the keyword from the first name; the
        // separator parser handles spacing between subsequent names.
        let ws = text[after..]
            .chars()
            .take_while(|c| c.is_whitespace())
            .map(char::len_utf8)
            .sum::<usize>();
        if ws == 0 {
            continue;
        }
        let mut cursor = after + ws;
        while let Some((name_start, name_end)) = consume_name_token(text, cursor) {
            cursor = name_end;
            let name = &text[name_start..name_end];
            // A name containing a slash is already a path, not a bare patch
            // name; emit nothing for it but continue scanning the list so a
            // trailing `, foo` still resolves.
            if !name.contains('/') {
                refs.push(FileRef {
                    start: name_start,
                    end: name_end,
                    path: format!("debian/patches/{name}"),
                });
            }
            // The singular form stops at the first token. The plural form
            // continues across `, ` / ` and ` / `, and ` separators.
            if !plural {
                break;
            }
            let Some(sep_end) = consume_list_separator(text, cursor) else {
                break;
            };
            cursor = sep_end;
        }

        search_from = cursor.max(search_from);
    }

    refs
}

/// Consume a single patch-name token starting at `start`, returning the
/// `(start, end)` byte range of the token (with trailing dots/slashes
/// trimmed), or `None` if no token is present.
fn consume_name_token(text: &str, start: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut cursor = start;
    while cursor < bytes.len() {
        let c = text[cursor..].chars().next().unwrap();
        if !is_path_char(c) {
            break;
        }
        cursor += c.len_utf8();
    }
    let mut end = cursor;
    while end > start && matches!(bytes[end - 1], b'.' | b'/') {
        end -= 1;
    }
    if end == start {
        None
    } else {
        Some((start, end))
    }
}

/// Consume a list separator (`, `, ` and `, `, and `) starting at `cursor`,
/// returning the byte offset just past the separator, or `None` if there is
/// no separator at this position.
fn consume_list_separator(text: &str, cursor: usize) -> Option<usize> {
    let rest = &text[cursor..];
    // `, and ` / `, ` / `,` (allow either order: comma optionally followed by
    // whitespace, optionally followed by `and` plus whitespace).
    if let Some(after_comma) = rest.strip_prefix(',') {
        let ws: usize = after_comma
            .chars()
            .take_while(|c| c.is_whitespace())
            .map(char::len_utf8)
            .sum();
        let after_ws = &after_comma[ws..];
        if let Some(after_and) = strip_word(after_ws, "and") {
            let ws2: usize = after_and
                .chars()
                .take_while(|c| c.is_whitespace())
                .map(char::len_utf8)
                .sum();
            if ws2 == 0 {
                return None;
            }
            return Some(cursor + 1 + ws + "and".len() + ws2);
        }
        if ws == 0 {
            return None;
        }
        return Some(cursor + 1 + ws);
    }
    // ` and `
    let ws: usize = rest
        .chars()
        .take_while(|c| c.is_whitespace())
        .map(char::len_utf8)
        .sum();
    if ws == 0 {
        return None;
    }
    let after_ws = &rest[ws..];
    let after_and = strip_word(after_ws, "and")?;
    let ws2: usize = after_and
        .chars()
        .take_while(|c| c.is_whitespace())
        .map(char::len_utf8)
        .sum();
    if ws2 == 0 {
        return None;
    }
    Some(cursor + ws + "and".len() + ws2)
}

/// Strip a case-insensitive whole word `word` from the start of `text`,
/// returning the remainder. Returns `None` if `text` doesn't start with the
/// word or if the word is glued to a following alphanumeric character.
fn strip_word<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    if text.len() < word.len() {
        return None;
    }
    if !text[..word.len()].eq_ignore_ascii_case(word) {
        return None;
    }
    let rest = &text[word.len()..];
    if matches!(rest.as_bytes().first(), Some(b) if b.is_ascii_alphanumeric()) {
        return None;
    }
    Some(rest)
}

/// Find the next standalone `patch` or `patches` word in `text`, returning
/// its byte offset and whether it was the plural form.
fn find_patch_word(text: &str) -> Option<(usize, bool)> {
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(rel) = lower[from..].find("patch") {
        let idx = from + rel;
        from = idx + "patch".len();
        // Reject when glued to a preceding word or path character, so the
        // `patch` in `prepatch` or the filename `foo.patch` isn't taken for
        // the prose word.
        let prev_ok = idx == 0 || !is_path_char(text[..idx].chars().next_back().unwrap());
        if !prev_ok {
            continue;
        }
        let after = idx + "patch".len();
        let plural = matches!(text.as_bytes().get(after), Some(b'e' | b'E'))
            && matches!(text.as_bytes().get(after + 1), Some(b's' | b'S'));
        let trailing = if plural { after + 2 } else { after };
        // Reject any further trailing letter/digit (e.g. `patched`, `patchesy`).
        let next_ok =
            !matches!(text.as_bytes().get(trailing), Some(b) if b.is_ascii_alphanumeric());
        if next_ok {
            return Some((idx, plural));
        }
    }
    None
}

/// Find the next `d/` or `debian/` prefix in `text`, returning its byte offset
/// and length. Prefers the longer `debian/` form when both start at the same
/// position.
fn find_prefix(text: &str) -> Option<(usize, usize)> {
    let debian = text.find("debian/");
    let short = text.find("d/");
    match (debian, short) {
        (Some(d), Some(s)) if d <= s => Some((d, "debian/".len())),
        (Some(_), Some(s)) => Some((s, "d/".len())),
        (Some(d), None) => Some((d, "debian/".len())),
        (None, Some(s)) => Some((s, "d/".len())),
        (None, None) => None,
    }
}

/// Normalise a raw `d/...` or `debian/...` mention to a `debian/`-rooted path.
///
/// Returns `None` if there is no component after the prefix.
fn normalise(raw: &str) -> Option<String> {
    let rest = raw
        .strip_prefix("debian/")
        .or_else(|| raw.strip_prefix("d/"))?;
    if rest.is_empty() {
        return None;
    }
    Some(format!("debian/{rest}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(text: &str) -> Vec<String> {
        iter_file_refs(text).into_iter().map(|r| r.path).collect()
    }

    #[test]
    fn detects_short_form() {
        assert_eq!(paths("d/control: Add foo"), vec!["debian/control"]);
    }

    #[test]
    fn detects_long_form() {
        assert_eq!(
            paths("debian/tests/control: Add foo"),
            vec!["debian/tests/control"]
        );
    }

    #[test]
    fn detects_patch_path() {
        assert_eq!(
            paths("d/patches/03_fix_protocol_v2_deepen_flush.patch: Remove patch."),
            vec!["debian/patches/03_fix_protocol_v2_deepen_flush.patch"]
        );
    }

    #[test]
    fn trims_trailing_dot() {
        assert_eq!(paths("d/tests/control. Add"), vec!["debian/tests/control"]);
    }

    #[test]
    fn detects_gbp_conf() {
        assert_eq!(
            paths("d/gbp.conf: Update debian-branch"),
            vec!["debian/gbp.conf"]
        );
    }

    #[test]
    fn span_covers_only_the_path() {
        let refs = iter_file_refs("Update d/control now");
        assert_eq!(refs.len(), 1);
        assert_eq!(
            &"Update d/control now"[refs[0].start..refs[0].end],
            "d/control"
        );
    }

    #[test]
    fn ignores_bare_prefix() {
        assert!(paths("nothing in d/ here").is_empty());
        assert!(paths("debian/ alone").is_empty());
    }

    #[test]
    fn ignores_glued_prefix() {
        // The `d/` here is part of a larger token, not a standalone mention.
        assert!(paths("see upstreamd/control").is_empty());
    }

    #[test]
    fn multiple_mentions() {
        assert_eq!(
            paths("d/control and d/rules updated"),
            vec!["debian/control", "debian/rules"]
        );
    }

    #[test]
    fn patches_subdir_without_extension() {
        assert_eq!(
            paths("d/patches/older-pyo3: No need"),
            vec!["debian/patches/older-pyo3"]
        );
    }

    fn patch_words(text: &str) -> Vec<String> {
        iter_patch_word_refs(text)
            .into_iter()
            .map(|r| r.path)
            .collect()
    }

    #[test]
    fn patch_word_with_extension() {
        assert_eq!(
            patch_words("Drop obsolete patch relax-pyo3.patch."),
            vec!["debian/patches/relax-pyo3.patch"]
        );
    }

    #[test]
    fn patch_word_extensionless_candidate() {
        // The heuristic accepts any token; existence is checked by the caller.
        assert_eq!(
            patch_words("Remove patch 03_fix"),
            vec!["debian/patches/03_fix"]
        );
    }

    #[test]
    fn patch_word_case_insensitive() {
        assert_eq!(
            patch_words("Patch foo.patch refreshed"),
            vec!["debian/patches/foo.patch"]
        );
    }

    #[test]
    fn patch_word_span_covers_name_only() {
        let refs = iter_patch_word_refs("Drop patch relax.patch.");
        assert_eq!(refs.len(), 1);
        assert_eq!(
            &"Drop patch relax.patch."[refs[0].start..refs[0].end],
            "relax.patch"
        );
    }

    #[test]
    fn patch_word_plural_matches_first_token() {
        // The plural form behaves like the singular: it yields a token whose
        // existence the caller validates. `for` won't resolve to a real file
        // in `debian/patches/`, so callers discard it.
        assert_eq!(
            patch_words("Refresh patches for the new release"),
            vec!["debian/patches/for"]
        );
    }

    #[test]
    fn patch_word_ignores_glued() {
        assert!(patch_words("a prepatch step").is_empty());
    }

    #[test]
    fn patch_word_skips_path_token() {
        // A token that's already a path is handled by `iter_file_refs`, so the
        // bare-name heuristic ignores it to avoid a duplicate.
        assert!(patch_words("see patch d/patches/foo.patch").is_empty());
    }

    #[test]
    fn patch_word_plural_comma_list() {
        assert_eq!(
            patch_words("Drop patches foo.patch, bar.patch, baz.patch."),
            vec![
                "debian/patches/foo.patch",
                "debian/patches/bar.patch",
                "debian/patches/baz.patch",
            ]
        );
    }

    #[test]
    fn patch_word_plural_and_pair() {
        assert_eq!(
            patch_words("Refresh patches foo.patch and bar.patch."),
            vec!["debian/patches/foo.patch", "debian/patches/bar.patch"]
        );
    }

    #[test]
    fn patch_word_plural_comma_and() {
        assert_eq!(
            patch_words("Refresh patches foo.patch, bar.patch and baz.patch."),
            vec![
                "debian/patches/foo.patch",
                "debian/patches/bar.patch",
                "debian/patches/baz.patch",
            ]
        );
    }

    #[test]
    fn patch_word_plural_oxford_comma() {
        assert_eq!(
            patch_words("Refresh patches foo.patch, bar.patch, and baz.patch."),
            vec![
                "debian/patches/foo.patch",
                "debian/patches/bar.patch",
                "debian/patches/baz.patch",
            ]
        );
    }

    #[test]
    fn patch_word_plural_spans_cover_each_name() {
        let text = "Drop patches a.patch, b.patch and c.patch.";
        let refs = iter_patch_word_refs(text);
        assert_eq!(refs.len(), 3);
        assert_eq!(&text[refs[0].start..refs[0].end], "a.patch");
        assert_eq!(&text[refs[1].start..refs[1].end], "b.patch");
        assert_eq!(&text[refs[2].start..refs[2].end], "c.patch");
    }
}
