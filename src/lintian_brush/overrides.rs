//! Buffer-aware variant of `LintianIssue::should_fix`.
//!
//! `LintianIssue::should_fix` reads `debian/source/lintian-overrides`
//! straight from disk. In an editor, the user might be in the middle of
//! adding an override and not have saved yet — we want the new override
//! to suppress its diagnostic immediately, not after a save.

use ::lintian_brush::LintianIssue;

use crate::debian_workspace::workspace::LspDebianWorkspace;

/// Honour open lintian-overrides buffers as well as on-disk overrides.
pub fn should_fix(ws: &LspDebianWorkspace<'_>, issue: &LintianIssue) -> bool {
    use ::lintian_brush::lintian_overrides::OverrideLineMatch as _;
    use lintian_overrides::{find_override_files, LintianOverrides};

    let base_path = ws.base_path();
    for path in find_override_files(base_path) {
        let rel = match path.strip_prefix(base_path) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let text = ws
            .current_text(rel)
            .unwrap_or_else(|| std::sync::Arc::from(""));
        let Ok(parsed) = LintianOverrides::parse(&text).ok() else {
            continue;
        };
        for line in parsed.lines() {
            if line.matches_issue(issue) {
                return false;
            }
        }
    }
    true
}
