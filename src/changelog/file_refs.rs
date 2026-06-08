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
//! - the prose form `patch <name>` (`iter_patch_word_refs`), which names a
//!   patch in `debian/patches/` without the directory prefix. This form is
//!   only meaningful when the named patch actually exists, so callers should
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

/// Iterate `patch <name>` candidates within `text`.
///
/// Matches the word `patch` (case-insensitive, not part of a larger word and
/// not the plural `patches`) followed by a single whitespace-delimited token,
/// yielding a [`FileRef`] whose `path` is `debian/patches/<token>` and whose
/// span covers just the token. Trailing sentence punctuation on the token is
/// trimmed.
///
/// This is a loose heuristic (`patch to fix the build` would match `to`), so
/// callers must check that the path exists before turning it into a link.
pub fn iter_patch_word_refs(text: &str) -> Vec<FileRef> {
    let bytes = text.as_bytes();
    let mut refs = Vec::new();
    let mut search_from = 0;

    while let Some(rel) = find_patch_word(&text[search_from..]) {
        let word_start = search_from + rel;
        let after = word_start + "patch".len();
        search_from = after;

        // Require whitespace separating the word from the name.
        let mut cursor = after;
        let ws = text[cursor..]
            .chars()
            .take_while(|c| c.is_whitespace())
            .map(char::len_utf8)
            .sum::<usize>();
        if ws == 0 {
            continue;
        }
        cursor += ws;

        // Consume the name token.
        let name_start = cursor;
        while cursor < bytes.len() && is_path_char(text[cursor..].chars().next().unwrap()) {
            cursor += text[cursor..].chars().next().unwrap().len_utf8();
        }
        let mut name_end = cursor;
        while name_end > name_start && matches!(bytes[name_end - 1], b'.' | b'/') {
            name_end -= 1;
        }
        if name_end == name_start {
            continue;
        }
        let name = &text[name_start..name_end];
        // A name containing a slash is already a path, not a bare patch name.
        if name.contains('/') {
            continue;
        }
        refs.push(FileRef {
            start: name_start,
            end: name_end,
            path: format!("debian/patches/{name}"),
        });
    }

    refs
}

/// Find the next standalone, lowercase-or-mixed-case `patch` word (not the
/// plural `patches`) in `text`, returning its byte offset.
fn find_patch_word(text: &str) -> Option<usize> {
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(rel) = lower[from..].find("patch") {
        let idx = from + rel;
        from = idx + "patch".len();
        // Reject when glued to a preceding word or path character, so the
        // `patch` in `prepatch` or the filename `foo.patch` isn't taken for
        // the prose word.
        let prev_ok = idx == 0 || !is_path_char(text[..idx].chars().next_back().unwrap());
        // Reject the plural `patches` and any other trailing letter/digit.
        let next_ok = !matches!(text.as_bytes().get(idx + "patch".len()), Some(b) if b.is_ascii_alphanumeric());
        if prev_ok && next_ok {
            return Some(idx);
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
    fn patch_word_ignores_plural() {
        assert!(patch_words("Refresh patches for the new release").is_empty());
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
}
