//! Debhelper-specific helpers for `debian/rules`.
//!
//! Recognising `dh_*` commands and the `override_dh_*` / `execute_*_dh_*`
//! target families is Debian packaging domain knowledge rather than generic
//! Makefile parsing, so it lives here rather than in the Makefile parser.

/// A `dh_*` command found in recipe text, with its absolute source range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DhCommand {
    /// Byte offset of the first character (`d`).
    pub start: u32,
    /// Byte offset past the last character.
    pub end: u32,
    /// The command name (e.g. `dh_install`).
    pub name: String,
}

/// Scan recipe text for `dh_*` debhelper command invocations.
///
/// `base` is the absolute byte offset of `text` in the source document, so the
/// returned ranges are absolute. Matches are anchored at word boundaries, so
/// `mydh_foo` does not match.
pub fn iter_dh_commands(text: &str, base: u32) -> Vec<DhCommand> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        if &bytes[i..i + 3] == b"dh_" && (i == 0 || !is_word_byte(bytes[i - 1])) {
            let start = i;
            let mut j = i + 3;
            while j < bytes.len() && is_word_byte(bytes[j]) {
                j += 1;
            }
            if j > i + 3 {
                out.push(DhCommand {
                    start: base + start as u32,
                    end: base + j as u32,
                    name: text[start..j].to_owned(),
                });
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Extract the debhelper command a rules target hooks into, if any.
///
/// `override_dh_auto_test` -> `dh_auto_test`,
/// `execute_before_dh_install` -> `dh_install`. Returns `None` for targets that
/// are not dh hooks.
pub fn command_for_target(target: &str) -> Option<&str> {
    let cmd = target
        .strip_prefix("override_")
        .or_else(|| target.strip_prefix("execute_before_"))
        .or_else(|| target.strip_prefix("execute_after_"))?;
    cmd.starts_with("dh_").then_some(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_for_target_extracts_hooks() {
        assert_eq!(
            command_for_target("override_dh_auto_test"),
            Some("dh_auto_test")
        );
        assert_eq!(
            command_for_target("execute_before_dh_install"),
            Some("dh_install")
        );
        assert_eq!(
            command_for_target("execute_after_dh_strip"),
            Some("dh_strip")
        );
        // Not a dh hook.
        assert_eq!(command_for_target("build"), None);
        assert_eq!(command_for_target("override_something"), None);
    }

    #[test]
    fn iter_dh_commands_basic() {
        let cmds = iter_dh_commands("dh_install --opt && dh_link", 0);
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].name, "dh_install");
        assert_eq!(cmds[1].name, "dh_link");
    }

    #[test]
    fn iter_dh_commands_word_boundary() {
        // `mydh_foo` shouldn't match.
        let cmds = iter_dh_commands("mydh_foo dh_real", 0);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].name, "dh_real");
    }
}
