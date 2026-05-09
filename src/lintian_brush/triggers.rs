use std::path::Path;

use ::lintian_brush::workspace::{ChangelogAspect, DetectorCost, Trigger, WatchAspect};
use debian_changelog::ChangeLog;

pub use crate::phase::RunPhase;

/// Maximum [`DetectorCost`] this phase will run.
pub(super) fn phase_max_cost(phase: RunPhase) -> DetectorCost {
    match phase {
        RunPhase::Keystroke => DetectorCost::Filesystem,
        RunPhase::Open => DetectorCost::Subprocess,
        RunPhase::Explicit => DetectorCost::Network,
    }
}

/// Whether detectors are allowed to use the network in this phase.
pub(super) fn phase_allow_net(phase: RunPhase) -> bool {
    matches!(phase, RunPhase::Explicit)
}

/// Pre-computed change context, built once per detector-running
/// call so each detector's triggers can be checked without re-walking
/// the rowan tree.
pub(super) struct ChangeContext<'a> {
    /// Set of (paragraph_key, field) pairs whose entries overlap any
    /// changed range. `None` means we don't have field-level info —
    /// triggers fall back to file-level match (whole-file scope).
    pub(super) deb822: Option<Deb822ChangeIndex>,
    pub(super) changelog: Option<&'a ChangeLog>,
    pub(super) yaml: Option<&'a yaml_edit::YamlFile>,
    pub(super) watch: Option<&'a debian_watch::parse::ParsedWatchFile>,
    pub(super) changed_ranges: Option<&'a [rowan::TextRange]>,
}

/// Decide whether a detector with the given `triggers` should run
/// for an edit on `rel`.
///
/// Takes the static triggers slice off `DetectorRegistration` so we
/// can decide *before* instantiating the detector — keeps us from
/// allocating ~150 `Box<dyn Detector>` values per keystroke just to
/// throw most of them away.
///
/// Filtering is conservative: a detector with no declared triggers
/// always runs, and a detector with at least one trigger that *might*
/// match runs even if other triggers definitely don't. We never block
/// a detector that the registry hasn't told us is irrelevant.
pub(super) fn triggers_match(
    triggers: &'static [Trigger],
    rel: &Path,
    ctx: &ChangeContext<'_>,
) -> bool {
    if triggers.is_empty() {
        // No declared triggers — be conservative and run it.
        return true;
    }
    triggers.iter().any(|t| trigger_matches(t, rel, ctx))
}

/// Does `trigger` match an edit on `rel`, given the pre-built
/// `ChangeContext`?  When the relevant parse is `None` we fall back
/// to the file-level match (over-trigger rather than miss).
fn trigger_matches(trigger: &Trigger, rel: &Path, ctx: &ChangeContext<'_>) -> bool {
    match trigger {
        Trigger::File(p) => Path::new(p) == rel,
        Trigger::Glob(g) => glob_matches(g, rel),
        Trigger::Deb822Field {
            file,
            paragraph_key,
            field,
        } => {
            if Path::new(file) != rel {
                return false;
            }
            let Some(index) = &ctx.deb822 else {
                // No index built — either no parse, or no changed-range
                // info to narrow on. Assume possible match.
                return true;
            };
            index.matches(paragraph_key, field)
        }
        Trigger::Watch(aspect) => {
            if rel != Path::new("debian/watch") {
                return false;
            }
            let Some(watch) = ctx.watch else {
                return true;
            };
            let Some(ranges) = ctx.changed_ranges else {
                return true;
            };
            ranges
                .iter()
                .any(|r| watch_range_touches_aspect(watch, *r, *aspect))
        }
        Trigger::Changelog(aspect) => {
            if rel != Path::new("debian/changelog") {
                return false;
            }
            let Some(changelog) = ctx.changelog else {
                return true;
            };
            let Some(ranges) = ctx.changed_ranges else {
                return true;
            };
            ranges
                .iter()
                .any(|r| changelog_range_touches_aspect(changelog, *r, *aspect))
        }
        Trigger::UpstreamMetadataField(field) => {
            if rel != Path::new("debian/upstream/metadata") {
                return false;
            }
            let Some(yaml) = ctx.yaml else {
                return true;
            };
            let Some(ranges) = ctx.changed_ranges else {
                return true;
            };
            ranges
                .iter()
                .any(|r| yaml_range_touches_top_field(yaml, *r, field))
        }
    }
}

/// Match an `fnmatch(3)`-style glob against `rel`. Supports `*` (any
/// run of non-separator chars) and `?` (single non-separator char);
/// no character classes, no `**`. Path separators are matched literally
/// — a `*` does not cross a `/`.
fn glob_matches(pattern: &str, rel: &Path) -> bool {
    let Some(s) = rel.to_str() else {
        return false;
    };
    glob_matches_str(pattern, s)
}

fn glob_matches_str(pattern: &str, s: &str) -> bool {
    let pat = pattern.as_bytes();
    let txt = s.as_bytes();
    glob_matches_inner(pat, 0, txt, 0)
}

fn glob_matches_inner(pat: &[u8], mut pi: usize, txt: &[u8], mut ti: usize) -> bool {
    while pi < pat.len() {
        match pat[pi] {
            b'*' => {
                // Skip runs of `*`.
                while pi < pat.len() && pat[pi] == b'*' {
                    pi += 1;
                }
                if pi == pat.len() {
                    // Trailing `*` — match anything but a `/`.
                    return !txt[ti..].contains(&b'/');
                }
                // Try every position in the current path component.
                while ti <= txt.len() {
                    if glob_matches_inner(pat, pi, txt, ti) {
                        return true;
                    }
                    if ti == txt.len() || txt[ti] == b'/' {
                        return false;
                    }
                    ti += 1;
                }
                return false;
            }
            b'?' => {
                if ti >= txt.len() || txt[ti] == b'/' {
                    return false;
                }
                pi += 1;
                ti += 1;
            }
            c => {
                if ti >= txt.len() || txt[ti] != c {
                    return false;
                }
                pi += 1;
                ti += 1;
            }
        }
    }
    ti == txt.len()
}

/// All `(paragraph_key, field_name)` pairs whose entries overlap any
/// of the changed ranges in a deb822 file. Built once per
/// detector-running call so each detector's `Trigger::Deb822Field`
/// triggers can be checked against a pre-walked set instead of
/// re-walking the rowan tree per trigger.
///
/// The cardinality is bounded by the number of (paragraph,
/// field) entries actually touched — for a single keystroke this is
/// typically 1, almost never more than a handful. Stores the
/// *identifying* keys present in each touched paragraph (so a
/// `Trigger::Deb822Field { paragraph_key: "Source", .. }` matches
/// any paragraph whose touched entries include `Source`).
#[derive(Default)]
pub(super) struct Deb822ChangeIndex {
    /// Pairs of (paragraph-identifying-key, touched-field-name).
    /// Both come from actual entry keys in the deb822 file, so they
    /// can be matched against trigger patterns (which support `*`
    /// wildcards) by walking this set once per trigger.
    entries: Vec<(String, String)>,
}

impl Deb822ChangeIndex {
    pub(super) fn build(deb822: &deb822_lossless::Deb822, ranges: &[rowan::TextRange]) -> Self {
        let mut entries: Vec<(String, String)> = Vec::new();
        for &range in ranges {
            for paragraph in deb822.paragraphs_in_range(range) {
                // Snapshot every key in the paragraph — these are the
                // candidate `paragraph_key` values that select this
                // paragraph as the trigger scope.
                let para_keys: Vec<String> = paragraph.entries().filter_map(|e| e.key()).collect();
                if para_keys.is_empty() {
                    continue;
                }
                for entry in paragraph.entries_in_range(range) {
                    let Some(touched_field) = entry.key() else {
                        continue;
                    };
                    for para_key in &para_keys {
                        let pair = (para_key.clone(), touched_field.clone());
                        if !entries.contains(&pair) {
                            entries.push(pair);
                        }
                    }
                }
            }
        }
        Deb822ChangeIndex { entries }
    }

    pub(super) fn matches(&self, paragraph_key: &str, field: &str) -> bool {
        self.entries
            .iter()
            .any(|(pk, f)| name_matches(paragraph_key, pk) && name_matches(field, f))
    }
}

/// Does the changed `range` touch any part of a changelog entry that
/// corresponds to `aspect`? Conservative: when in doubt, return true.
fn changelog_range_touches_aspect(
    changelog: &ChangeLog,
    range: rowan::TextRange,
    aspect: ChangelogAspect,
) -> bool {
    use rowan::ast::AstNode as _;
    for entry in changelog.entries_in_range(range) {
        let entry_range = entry.syntax().text_range();
        if !ranges_overlap(entry_range, range) {
            continue;
        }
        // Determine whether the changed range falls in the header,
        // body, or footer. Without finer-grained API, treat any
        // changelog edit as potentially touching the aspect — but we
        // can rule out body-only edits against header-aspect triggers.
        match aspect {
            ChangelogAspect::Body => {
                // Any overlap with an entry plausibly touches the body.
                return true;
            }
            ChangelogAspect::Version | ChangelogAspect::Distribution | ChangelogAspect::Urgency => {
                if let Some(header) = entry.header() {
                    if ranges_overlap(header.syntax().text_range(), range) {
                        return true;
                    }
                }
            }
            ChangelogAspect::Maintainer | ChangelogAspect::Timestamp => {
                if let Some(footer) = entry.footer() {
                    if ranges_overlap(footer.syntax().text_range(), range) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Does the changed `range` overlap any part of the watch file that
/// corresponds to `aspect`? Conservative: when in doubt, return true.
fn watch_range_touches_aspect(
    watch: &debian_watch::parse::ParsedWatchFile,
    range: rowan::TextRange,
    aspect: WatchAspect,
) -> bool {
    match aspect {
        WatchAspect::Version => watch
            .version_range()
            .map(|r| ranges_overlap(r, range))
            .unwrap_or(false),
        WatchAspect::Source => watch.entries().any(|e| {
            e.url_range()
                .map(|r| ranges_overlap(r, range))
                .unwrap_or(false)
        }),
        WatchAspect::MatchingPattern => watch.entries().any(|e| {
            e.matching_pattern_range()
                .map(|r| ranges_overlap(r, range))
                .unwrap_or(false)
        }),
        WatchAspect::Template(kind) => watch.entries().any(|e| {
            let Some(template_range) = e.template_range() else {
                return false;
            };
            if !ranges_overlap(template_range, range) {
                return false;
            }
            // A bare "*" matches any kind.
            if kind == "*" {
                return true;
            }
            e.template_kind().as_deref() == Some(kind)
        }),
        WatchAspect::Option(name) => watch.entries().any(|e| {
            e.option_range(name)
                .map(|r| ranges_overlap(r, range))
                .unwrap_or(false)
        }),
    }
}

/// Does the changed `range` touch a top-level mapping entry in `yaml`
/// whose key matches `field`? `field` follows Trigger's wildcard rules.
fn yaml_range_touches_top_field(
    yaml: &yaml_edit::YamlFile,
    range: rowan::TextRange,
    field: &str,
) -> bool {
    use rowan::ast::AstNode as _;
    let Some(doc) = yaml.document() else {
        return false;
    };
    let Some(mapping) = doc.as_mapping() else {
        return false;
    };
    for entry in mapping.entries() {
        let entry_range = entry.syntax().text_range();
        if !ranges_overlap(entry_range, range) {
            continue;
        }
        let Some(key_node) = entry.key_node() else {
            continue;
        };
        let key = match &key_node {
            yaml_edit::YamlNode::Scalar(s) => s.as_string(),
            _ => continue,
        };
        if name_matches(field, &key) {
            return true;
        }
    }
    false
}

/// Wildcard match for trigger field-/paragraph-key patterns: a bare
/// `*` matches anything, a trailing `*` is a prefix match, otherwise
/// equality.
fn name_matches(pattern: &str, name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return name.starts_with(prefix);
    }
    pattern == name
}

fn ranges_overlap(a: rowan::TextRange, b: rowan::TextRange) -> bool {
    a.start() < b.end() && b.start() < a.end()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_glob_matches_str_literal() {
        assert!(glob_matches_str("debian/control", "debian/control"));
        assert!(!glob_matches_str("debian/control", "debian/changelog"));
    }

    #[test]
    fn test_glob_matches_star() {
        assert!(glob_matches_str("debian/*", "debian/control"));
        assert!(glob_matches_str("debian/*", "debian/changelog"));
        assert!(!glob_matches_str("debian/*", "debian/subdir/file"));
    }

    #[test]
    fn test_glob_matches_question() {
        assert!(glob_matches_str("debian/?ontrol", "debian/control"));
        assert!(!glob_matches_str("debian/?ontrol", "debian/xontrol2"));
    }

    #[test]
    fn test_glob_matches_path() {
        assert!(glob_matches(
            "debian/patches/*",
            Path::new("debian/patches/fix-bug.patch")
        ));
        assert!(!glob_matches(
            "debian/patches/*",
            Path::new("debian/patches/subdir/fix.patch")
        ));
    }

    #[test]
    fn test_name_matches_exact() {
        assert!(name_matches("Source", "Source"));
        assert!(!name_matches("Source", "Package"));
    }

    #[test]
    fn test_name_matches_wildcard() {
        assert!(name_matches("*", "anything"));
        assert!(name_matches("Build-Depends*", "Build-Depends"));
        assert!(name_matches("Build-Depends*", "Build-Depends-Indep"));
        assert!(!name_matches("Build-Depends*", "Depends"));
    }
}
