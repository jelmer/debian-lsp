//! Detection and linking of GHSA identifiers.
//!
//! GitHub Security Advisory identifiers (e.g. `GHSA-jfh8-c2jp-5v3q`) appear in
//! changelog detail lines as plain prose, like CVE references. This module finds
//! them so the LSP can highlight them and link to the GitHub Advisory Database,
//! and the SCIP indexer can record them. Unlike CVEs, there is no UDD lookup, so
//! a plain link to the advisory page is all that is available.

/// A GHSA identifier found in a span of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhsaRef {
    /// The canonical identifier, e.g. `GHSA-jfh8-c2jp-5v3q`.
    pub id: String,
    /// Byte offset of the start of the match within the scanned text.
    pub start: usize,
    /// Byte offset of the end of the match within the scanned text.
    pub end: usize,
}

/// URL of the GitHub Advisory Database page for a GHSA identifier.
pub fn advisory_url(id: &str) -> String {
    format!("https://github.com/advisories/{id}")
}

/// Minimal markdown for a GHSA: a linked identifier pointing at the GitHub
/// Advisory Database. Used for the LSP hover and the SCIP indexer's static
/// documentation.
pub fn link_markdown(id: &str) -> String {
    format!("**[{}]({})**", id, advisory_url(id))
}

/// Characters permitted in a GHSA identifier group, per the GHSA syntax
/// (`GHSA-xxxx-xxxx-xxxx` where each `x` is from `23456789cfghjmpqrvwx`).
fn is_ghsa_char(b: u8) -> bool {
    matches!(
        b.to_ascii_lowercase(),
        b'2'..=b'9'
            | b'c'
            | b'f'
            | b'g'
            | b'h'
            | b'j'
            | b'm'
            | b'p'
            | b'q'
            | b'r'
            | b'v'
            | b'w'
            | b'x'
    )
}

/// Find all GHSA identifiers in `text`.
///
/// Matches `GHSA-` (case-insensitive) followed by three hyphen-separated groups
/// of exactly four characters from the GHSA alphabet. The returned `id` is
/// normalised to lower-case, the canonical form.
pub fn find_ghsas(text: &str) -> Vec<GhsaRef> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut search_from = 0;

    while let Some(rel) = find_ghsa_prefix(&text[search_from..]) {
        let start = search_from + rel;
        // Skip the 5-char "GHSA-" prefix.
        let mut pos = start + 5;
        let mut groups = 0;
        let mut ok = true;

        loop {
            let group_start = pos;
            while pos < bytes.len() && is_ghsa_char(bytes[pos]) {
                pos += 1;
            }
            if pos - group_start != 4 {
                ok = false;
                break;
            }
            groups += 1;
            if groups == 3 {
                break;
            }
            // Groups after the first are separated by a hyphen.
            if pos >= bytes.len() || bytes[pos] != b'-' {
                ok = false;
                break;
            }
            pos += 1;
        }

        if !ok {
            search_from = start + 5;
            continue;
        }

        let id = text[start..pos].to_ascii_lowercase();
        out.push(GhsaRef {
            id,
            start,
            end: pos,
        });
        search_from = pos;
    }

    out
}

/// Find the next case-insensitive `GHSA-` prefix in `text`, returning its byte
/// offset.
fn find_ghsa_prefix(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    bytes.windows(5).position(|w| {
        w[0].eq_ignore_ascii_case(&b'G')
            && w[1].eq_ignore_ascii_case(&b'H')
            && w[2].eq_ignore_ascii_case(&b'S')
            && w[3].eq_ignore_ascii_case(&b'A')
            && w[4] == b'-'
    })
}

/// Return the GHSA reference covering byte `offset` within `text`, if any.
pub fn ghsa_at_offset(text: &str, offset: usize) -> Option<GhsaRef> {
    find_ghsas(text)
        .into_iter()
        .find(|g| offset >= g.start && offset <= g.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_single_ghsa() {
        let ghsas = find_ghsas("Fix GHSA-jfh8-c2jp-5v3q in the parser.");
        assert_eq!(
            ghsas,
            vec![GhsaRef {
                id: "ghsa-jfh8-c2jp-5v3q".to_string(),
                start: 4,
                end: 23,
            }]
        );
    }

    #[test]
    fn finds_multiple_ghsas() {
        let ghsas = find_ghsas("GHSA-jfh8-c2jp-5v3q and GHSA-9h6g-mxqv-vw5c.");
        let ids: Vec<_> = ghsas.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, vec!["ghsa-jfh8-c2jp-5v3q", "ghsa-9h6g-mxqv-vw5c"]);
    }

    #[test]
    fn uppercase_is_normalised() {
        let ghsas = find_ghsas("GHSA-JFH8-C2JP-5V3Q");
        assert_eq!(ghsas.len(), 1);
        assert_eq!(ghsas[0].id, "ghsa-jfh8-c2jp-5v3q");
    }

    #[test]
    fn rejects_short_group() {
        assert!(find_ghsas("GHSA-jfh-c2jp-5v3q").is_empty());
    }

    #[test]
    fn rejects_too_few_groups() {
        assert!(find_ghsas("GHSA-jfh8-c2jp").is_empty());
    }

    #[test]
    fn rejects_char_outside_alphabet() {
        // 'l', 'o', 'i', 'u' are not in the GHSA alphabet.
        assert!(find_ghsas("GHSA-jfl8-c2jp-5v3q").is_empty());
    }

    #[test]
    fn stops_at_three_groups() {
        // A fourth group is not part of the identifier; only the first three
        // groups are matched, regardless of trailing hyphenated text.
        let ghsas = find_ghsas("GHSA-jfh8-c2jp-5v3q-extra");
        assert_eq!(ghsas.len(), 1);
        assert_eq!(ghsas[0].id, "ghsa-jfh8-c2jp-5v3q");
        assert_eq!(ghsas[0].end, 19);
    }

    #[test]
    fn no_false_positive_on_plain_text() {
        assert!(find_ghsas("This is a regular changelog entry.").is_empty());
    }

    #[test]
    fn ghsa_at_offset_hits_inside() {
        let text = "Fix GHSA-jfh8-c2jp-5v3q now.";
        let pos = text.find("c2jp").unwrap();
        assert_eq!(
            ghsa_at_offset(text, pos).map(|g| g.id),
            Some("ghsa-jfh8-c2jp-5v3q".to_string())
        );
    }

    #[test]
    fn ghsa_at_offset_misses_outside() {
        let text = "Fix GHSA-jfh8-c2jp-5v3q now.";
        assert_eq!(ghsa_at_offset(text, 0), None);
    }

    #[test]
    fn advisory_url_points_at_github() {
        assert_eq!(
            advisory_url("ghsa-jfh8-c2jp-5v3q"),
            "https://github.com/advisories/ghsa-jfh8-c2jp-5v3q"
        );
    }

    #[test]
    fn link_markdown_is_an_advisory_link() {
        assert_eq!(
            link_markdown("ghsa-jfh8-c2jp-5v3q"),
            "**[ghsa-jfh8-c2jp-5v3q](https://github.com/advisories/ghsa-jfh8-c2jp-5v3q)**"
        );
    }
}
