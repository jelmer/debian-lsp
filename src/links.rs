//! Detect web URLs embedded in prose.
//!
//! Packaging metadata mixes structured URL fields (where the whole value is a
//! URL) with free-form prose fields like `Comment` and `Disclaimer` in
//! `debian/copyright` that may mention `http(s)://` links inline. This module
//! finds those embedded URLs in a text span and returns their byte ranges, so
//! both the LSP (document links) and the SCIP indexer can surface them as
//! clickable links from one implementation.

/// Characters that may terminate a URL when it is followed by punctuation, so a
/// trailing `,`, `.`, `)` etc. in prose is not swallowed into the link.
fn trim_trailing(url: &str) -> &str {
    url.trim_end_matches([',', '.', ';', ':', ')', ']', '}', '>', '"', '\'', '!', '?'])
}

/// Whether `c` is allowed inside a URL we detect. Whitespace and angle brackets
/// always terminate; everything else is accepted and trailing punctuation is
/// trimmed afterwards by [`trim_trailing`].
fn is_url_char(c: char) -> bool {
    !c.is_whitespace() && c != '<' && c != '>'
}

/// Find `http(s)://` URLs in `text` and return their byte ranges (relative to
/// `text`), each trimmed of trailing punctuation.
pub fn find_urls(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut from = 0;
    while let Some(rel) = next_scheme(&text[from..]) {
        let start = from + rel;
        // A URL must start at a word boundary, not mid-token, so the `http://`
        // inside `xhttp://` is not matched.
        let preceded_by_word = start > 0
            && bytes
                .get(start - 1)
                .map(|&b| (b as char).is_alphanumeric())
                .unwrap_or(false);
        let mut end = start;
        while let Some(ch) = text[end..].chars().next() {
            if !is_url_char(ch) {
                break;
            }
            end += ch.len_utf8();
        }
        let url = trim_trailing(&text[start..end]);
        let real_end = start + url.len();
        // Require something after the scheme, so a bare `https://` is ignored.
        if !preceded_by_word && real_end > start + scheme_len(&text[start..]) {
            ranges.push((start, real_end));
        }
        from = end.max(start + 1);
    }
    ranges
}

/// Byte offset of the next `http://` or `https://` in `text`, whichever comes
/// first.
fn next_scheme(text: &str) -> Option<usize> {
    match (text.find("http://"), text.find("https://")) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Length of the scheme prefix (`http://` or `https://`) at the start of `text`.
fn scheme_len(text: &str) -> usize {
    if text.starts_with("https://") {
        "https://".len()
    } else {
        "http://".len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_bare_url() {
        assert_eq!(find_urls("https://example.org/x"), vec![(0, 21)]);
    }

    #[test]
    fn trims_trailing_punctuation() {
        let text = "see https://example.org/x, and more";
        let (s, e) = find_urls(text)[0];
        assert_eq!(&text[s..e], "https://example.org/x");
    }

    #[test]
    fn ignores_mid_token_scheme() {
        assert!(find_urls("xhttps://example.org").is_empty());
    }

    #[test]
    fn ignores_bare_scheme() {
        assert!(find_urls("https:// and http://").is_empty());
    }

    #[test]
    fn finds_multiple() {
        let text = "https://a.example/ and http://b.example/";
        let urls: Vec<_> = find_urls(text)
            .into_iter()
            .map(|(s, e)| &text[s..e])
            .collect();
        assert_eq!(urls, vec!["https://a.example/", "http://b.example/"]);
    }

    #[test]
    fn finds_url_embedded_in_prose() {
        let text = "See the upstream notice at https://example.org/notice for details.";
        let urls: Vec<_> = find_urls(text)
            .into_iter()
            .map(|(s, e)| &text[s..e])
            .collect();
        assert_eq!(urls, vec!["https://example.org/notice"]);
    }
}
