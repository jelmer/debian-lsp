//! Parsing helpers for DEP-3 patch headers.
//!
//! Thin wrappers over the [`dep3`] crate's relaxed parser: a quilt
//! patch under `debian/patches/` is a DEP-3 header (deb822) followed by
//! an unspecified body (usually a unified diff, sometimes an `Index:`
//! line, sometimes a bare `---`). [`dep3::lossless::header_end`] finds
//! that boundary; [`dep3::lossless::PatchHeader::parse_relaxed`] parses
//! the header portion and returns the offset where the body starts so
//! callers can map source ranges back into the original file.

#[cfg(any(feature = "lintian-brush", feature = "multiarch-hints"))]
pub use dep3::lossless::PatchHeader;

/// Parse the DEP-3 header portion of `content`. Returns the parsed
/// header and the byte offset where the diff body starts (equal to
/// `content.len()` if there is no body). Returns `None` if the header
/// portion can't be parsed as deb822 — e.g. a malformed continuation
/// line in the header.
#[cfg(any(feature = "lintian-brush", feature = "multiarch-hints"))]
pub fn parse_dep3_header(content: &str) -> Option<(PatchHeader, usize)> {
    PatchHeader::parse_relaxed(content).ok()
}

#[cfg(all(
    test,
    any(feature = "lintian-brush", feature = "multiarch-hints")
))]
mod tests {
    use super::*;

    #[test]
    fn parse_header_reads_first_paragraph() {
        let s = "Author: alice\nDescription: bla\n---\n@@ -1 +1 @@\n";
        let (header, end) = parse_dep3_header(s).expect("header parses");
        assert_eq!(end, "Author: alice\nDescription: bla\n".len());
        assert_eq!(header.author(), Some("alice".to_string()));
        assert_eq!(header.description(), Some("bla".to_string()));
    }

    #[test]
    fn parse_returns_header_end() {
        let s = "Author: alice\nDescription: bla\n---\n@@ -1 +1 @@\n-x\n+y\n";
        let (_, end) = parse_dep3_header(s).expect("parses");
        assert_eq!(end, "Author: alice\nDescription: bla\n".len());
    }
}
