//! Detection and lookup of CVE identifiers.
//!
//! CVE references (e.g. `CVE-2024-1234`) appear in changelog detail lines as
//! plain prose, unlike `Closes: #NNN` bug references which carry a keyword.
//! This module finds them so the LSP can highlight them and show hover details,
//! and the SCIP indexer can record them. Details (description, scope, affected
//! releases) are looked up from UDD's `security_issues` tables and cached; a
//! plain link to the Debian Security Tracker is the fallback when no data is
//! available.

mod cache;

pub use cache::{cve_summary, new_shared_cve_cache, CveSummary, SharedCveCache};

/// A CVE identifier found in a span of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CveRef {
    /// The canonical identifier, e.g. `CVE-2024-1234`.
    pub id: String,
    /// Byte offset of the start of the match within the scanned text.
    pub start: usize,
    /// Byte offset of the end of the match within the scanned text.
    pub end: usize,
}

/// URL of the Debian Security Tracker page for a CVE.
pub fn tracker_url(id: &str) -> String {
    format!("https://security-tracker.debian.org/tracker/{id}")
}

/// Minimal markdown for a CVE: a linked identifier pointing at the Debian
/// Security Tracker. Used as the fallback when no UDD data is available, and as
/// the SCIP indexer's static documentation.
pub fn link_markdown(id: &str) -> String {
    format!("**[{}]({})**", id, tracker_url(id))
}

/// Render a [`CveSummary`] as markdown: a linked identifier, description, scope,
/// any linked Debian bug, and per-release status. Shared by the LSP hover and
/// the SCIP indexer's symbol documentation.
pub fn summary_markdown(summary: &CveSummary) -> String {
    let mut lines = vec![link_markdown(&summary.id)];

    if let Some(description) = summary.description.as_deref() {
        if !description.is_empty() {
            lines.push(description.to_owned());
        }
    }
    if let Some(scope) = summary.scope.as_deref() {
        if !scope.is_empty() {
            lines.push(format!("**Scope:** {scope}"));
        }
    }
    if let Some(bug) = summary.bug {
        lines.push(format!(
            "**Debian bug:** [#{bug}](https://bugs.debian.org/{bug})"
        ));
    }
    if !summary.releases.is_empty() {
        let mut status_line = String::from("**Releases:**");
        for r in &summary.releases {
            let status = r.status.as_deref().unwrap_or("unknown");
            status_line.push_str(&format!("\n- {}: {}", r.release, status));
            if let Some(fixed) = r.fixed_version.as_deref() {
                if !fixed.is_empty() {
                    status_line.push_str(&format!(" (fixed in {fixed})"));
                }
            }
        }
        lines.push(status_line);
    }

    lines.join("\n\n")
}

/// Find all CVE identifiers in `text`.
///
/// Matches `CVE-` (case-insensitive) followed by a four-digit year, a hyphen,
/// and at least four digits, per the CVE identifier syntax. The returned `id`
/// is normalised to upper-case `CVE`; the year and sequence digits are kept
/// verbatim.
pub fn find_cves(text: &str) -> Vec<CveRef> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut search_from = 0;

    while let Some(rel) = find_cve_prefix(&text[search_from..]) {
        let start = search_from + rel;
        // Skip the 4-char "CVE-" prefix.
        let mut pos = start + 4;

        // Year: exactly four digits.
        let year_start = pos;
        while pos < bytes.len() && bytes[pos].is_ascii_digit() {
            pos += 1;
        }
        if pos - year_start != 4 || pos >= bytes.len() || bytes[pos] != b'-' {
            search_from = start + 4;
            continue;
        }
        pos += 1; // consume the hyphen

        // Sequence: at least four digits.
        let seq_start = pos;
        while pos < bytes.len() && bytes[pos].is_ascii_digit() {
            pos += 1;
        }
        if pos - seq_start < 4 {
            search_from = start + 4;
            continue;
        }

        let id = format!("CVE{}", &text[start + 3..pos]);
        out.push(CveRef {
            id,
            start,
            end: pos,
        });
        search_from = pos;
    }

    out
}

/// Find the next case-insensitive `CVE-` prefix in `text`, returning its byte
/// offset.
fn find_cve_prefix(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    bytes.windows(4).position(|w| {
        w[0].eq_ignore_ascii_case(&b'C')
            && w[1].eq_ignore_ascii_case(&b'V')
            && w[2].eq_ignore_ascii_case(&b'E')
            && w[3] == b'-'
    })
}

/// Return the CVE reference covering byte `offset` within `text`, if any.
pub fn cve_at_offset(text: &str, offset: usize) -> Option<CveRef> {
    find_cves(text)
        .into_iter()
        .find(|c| offset >= c.start && offset <= c.end)
}

#[cfg(test)]
mod tests {
    use super::cache::CveReleaseStatus;
    use super::*;

    #[test]
    fn finds_single_cve() {
        let cves = find_cves("Fix CVE-2024-1234 in the parser.");
        assert_eq!(
            cves,
            vec![CveRef {
                id: "CVE-2024-1234".to_string(),
                start: 4,
                end: 17,
            }]
        );
    }

    #[test]
    fn finds_multiple_cves() {
        let cves = find_cves("Fixes CVE-2024-1234 and CVE-2023-99999.");
        let ids: Vec<_> = cves.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["CVE-2024-1234", "CVE-2023-99999"]);
    }

    #[test]
    fn lowercase_prefix_is_normalised() {
        let cves = find_cves("cve-2024-1234");
        assert_eq!(cves.len(), 1);
        assert_eq!(cves[0].id, "CVE-2024-1234");
    }

    #[test]
    fn long_sequence_allowed() {
        let cves = find_cves("CVE-2024-1234567");
        assert_eq!(cves.len(), 1);
        assert_eq!(cves[0].id, "CVE-2024-1234567");
    }

    #[test]
    fn rejects_short_sequence() {
        assert!(find_cves("CVE-2024-123").is_empty());
    }

    #[test]
    fn rejects_short_year() {
        assert!(find_cves("CVE-204-1234").is_empty());
    }

    #[test]
    fn rejects_missing_hyphen() {
        assert!(find_cves("CVE-20241234").is_empty());
    }

    #[test]
    fn no_false_positive_on_plain_text() {
        assert!(find_cves("This is a regular changelog entry.").is_empty());
    }

    #[test]
    fn cve_at_offset_hits_inside() {
        let text = "Fix CVE-2024-1234 now.";
        let pos = text.find("2024").unwrap();
        assert_eq!(
            cve_at_offset(text, pos).map(|c| c.id),
            Some("CVE-2024-1234".to_string())
        );
    }

    #[test]
    fn cve_at_offset_misses_outside() {
        let text = "Fix CVE-2024-1234 now.";
        assert_eq!(cve_at_offset(text, 0), None);
    }

    #[test]
    fn tracker_url_points_at_security_tracker() {
        assert_eq!(
            tracker_url("CVE-2024-1234"),
            "https://security-tracker.debian.org/tracker/CVE-2024-1234"
        );
    }

    #[test]
    fn link_markdown_is_a_tracker_link() {
        assert_eq!(
            link_markdown("CVE-2024-1234"),
            "**[CVE-2024-1234](https://security-tracker.debian.org/tracker/CVE-2024-1234)**"
        );
    }

    #[test]
    fn summary_markdown_renders_all_fields() {
        let summary = CveSummary {
            id: "CVE-2021-44228".to_string(),
            description: Some("Apache Log4j2 remote code execution.".to_string()),
            scope: Some("remote".to_string()),
            bug: Some(1001478),
            releases: vec![
                CveReleaseStatus {
                    release: "bookworm".to_string(),
                    status: Some("resolved".to_string()),
                    fixed_version: Some("2.15.0-1".to_string()),
                },
                CveReleaseStatus {
                    release: "sid".to_string(),
                    status: Some("resolved".to_string()),
                    fixed_version: None,
                },
            ],
        };
        assert_eq!(
            summary_markdown(&summary),
            "**[CVE-2021-44228](https://security-tracker.debian.org/tracker/CVE-2021-44228)**\n\
             \n\
             Apache Log4j2 remote code execution.\n\
             \n\
             **Scope:** remote\n\
             \n\
             **Debian bug:** [#1001478](https://bugs.debian.org/1001478)\n\
             \n\
             **Releases:**\n\
             - bookworm: resolved (fixed in 2.15.0-1)\n\
             - sid: resolved"
        );
    }

    #[test]
    fn summary_markdown_minimal_is_just_the_link() {
        let summary = CveSummary {
            id: "CVE-2024-0001".to_string(),
            description: None,
            scope: None,
            bug: None,
            releases: vec![],
        };
        assert_eq!(
            summary_markdown(&summary),
            "**[CVE-2024-0001](https://security-tracker.debian.org/tracker/CVE-2024-0001)**"
        );
    }
}
