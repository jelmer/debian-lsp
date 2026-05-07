//! Known DEP-3 field names and descriptions.
//!
//! Sourced from the DEP-3 specification:
//! <https://dep-team.pages.debian.net/deps/dep3/>

use crate::deb822::completion::FieldInfo;

/// Canonical DEP-3 patch-header field names with one-line descriptions.
pub const DEP3_FIELDS: &[FieldInfo] = &[
    FieldInfo::new(
        "Description",
        "Synopsis on the first line followed by a longer indented body. Required (or `Subject` as an alias).",
    ),
    FieldInfo::new(
        "Subject",
        "Alias for `Description` carried over from email/git-format-patch headers.",
    ),
    FieldInfo::new(
        "Origin",
        "Where the patch came from. Optional category prefix (`upstream`, `backport`, `vendor`, `other`) followed by a comma and a URL or commit reference.",
    ),
    FieldInfo::new(
        "Bug",
        "URL of the upstream bug entry that this patch fixes.",
    ),
    FieldInfo::new(
        "Bug-Debian",
        "URL of the matching Debian bug entry. Other vendors use `Bug-<Vendor>` (e.g. `Bug-Ubuntu`).",
    ),
    FieldInfo::new(
        "Forwarded",
        "Whether the patch has been forwarded upstream. One of `yes`, `no`, `not-needed`, or a URL pointing at the forwarded patch.",
    ),
    FieldInfo::new(
        "Author",
        "Author of the patch. Equivalent to `From` (kept verbatim from `git format-patch` headers); both are accepted but `Author` is the DEP-3 canonical name.",
    ),
    FieldInfo::new(
        "From",
        "Alias for `Author` carried over from email/git-format-patch headers.",
    ),
    FieldInfo::new(
        "Reviewed-By",
        "Reviewer who has examined the patch. May appear multiple times.",
    ),
    FieldInfo::new(
        "Last-Update",
        "ISO date (`YYYY-MM-DD`) of the last edit to the patch metadata.",
    ),
    FieldInfo::new(
        "Applied-Upstream",
        "Set when the patch has landed upstream. Format: a version, a commit identifier, or a `commit:<id>` reference.",
    ),
];

/// Look up the canonical casing for a DEP-3 field name. Returns
/// `None` for fields not in the spec (typically vendor-specific
/// `Bug-<Vendor>` headers, or `X-`-prefixed extensions).
pub fn get_standard_field_name(field_name: &str) -> Option<&'static str> {
    crate::deb822::completion::get_standard_field_name(DEP3_FIELDS, field_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_casing_canonicalises_lowercase() {
        assert_eq!(get_standard_field_name("description"), Some("Description"));
        assert_eq!(get_standard_field_name("LAST-UPDATE"), Some("Last-Update"));
        assert_eq!(get_standard_field_name("Author"), Some("Author"));
    }

    #[test]
    fn standard_casing_returns_none_for_unknown() {
        assert_eq!(get_standard_field_name("Bug-Ubuntu"), None);
        assert_eq!(get_standard_field_name("X-Custom"), None);
    }
}
