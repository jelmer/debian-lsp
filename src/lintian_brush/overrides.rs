//! Buffer-aware lintian-override lookup.
//!
//! `LintianIssue::should_fix` reads `debian/source/lintian-overrides`
//! straight from disk. In an editor, the user might be in the middle of
//! adding an override and not have saved yet — we want the new override
//! to suppress its diagnostic immediately, not after a save.

use std::path::PathBuf;

use ::lintian_brush::LintianIssue;

use crate::debian_workspace::workspace::LspDebianWorkspace;

/// The override line that suppresses an issue, identified exactly enough
/// to address it with a [`DropLine`](::debian_workspace::action::LintianOverridesAction::DropLine).
///
/// `matches_issue` is lenient (a bare `tag` line overrides that tag for
/// every package), but `DropLine` matches its selector exactly. So the
/// fields here are read off the matched override line verbatim, not
/// copied from the issue.
///
/// `serde`-serialisable so it can ride on the diagnostic `data` field and
/// be reconstructed in `code_action` without re-scanning the overrides.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct OverrideMatch {
    /// Override file, relative to the package root (e.g.
    /// `debian/source/lintian-overrides`).
    pub file: PathBuf,
    /// Tag named on the override line.
    pub tag: String,
    /// Info text on the override line, if any.
    pub info: Option<String>,
    /// Package name from the line's `package:` prefix, if any.
    pub package: Option<String>,
}

/// Result of looking up whether an issue is suppressed by a lintian
/// override, honouring open (unsaved) override buffers.
pub enum OverrideStatus {
    /// No override matches the issue; the diagnostic should be surfaced
    /// as a normal, fixable problem.
    NotOverridden,
    /// An override matches the issue. The [`OverrideMatch`] identifies
    /// the exact line so a "remove override" code action can drop it.
    Overridden(OverrideMatch),
}

/// Look up whether `issue` is suppressed by a lintian override, checking
/// open buffers as well as on-disk override files.
pub fn override_status(ws: &LspDebianWorkspace<'_>, issue: &LintianIssue) -> OverrideStatus {
    use ::lintian_brush::lintian_overrides::OverrideLineMatch as _;
    use lintian_overrides::{find_override_files, LintianOverrides};

    let base_path = ws.base_path();
    for path in find_override_files(base_path) {
        let Ok(rel) = path.strip_prefix(base_path) else {
            continue;
        };
        let text = ws
            .current_text(rel)
            .unwrap_or_else(|| std::sync::Arc::from(""));
        let Ok(parsed) = LintianOverrides::parse(&text).ok() else {
            continue;
        };
        for line in parsed.lines() {
            if line.matches_issue(issue) {
                // A line without a tag can't match `matches_issue`, but
                // guard anyway so the selector is always well-formed.
                let Some(tag) = line.tag().map(|t| t.text().to_string()) else {
                    continue;
                };
                return OverrideStatus::Overridden(OverrideMatch {
                    file: rel.to_path_buf(),
                    tag,
                    info: line.info(),
                    package: line.package(),
                });
            }
        }
    }
    OverrideStatus::NotOverridden
}

/// Honour open lintian-overrides buffers as well as on-disk overrides.
///
/// Returns `false` when an override suppresses the issue.
pub fn should_fix(ws: &LspDebianWorkspace<'_>, issue: &LintianIssue) -> bool {
    matches!(override_status(ws, issue), OverrideStatus::NotOverridden)
}
