//! Clickable URL links for deb822-based files (`debian/control`,
//! `debian/copyright`).
//!
//! Fields are classified on [`FieldInfo`]: a [`FieldContent::Url`] field's whole
//! value is one link, while a [`FieldContent::Prose`] field is scanned for URLs
//! embedded in its text with [`crate::links::find_urls`]. The same field
//! metadata and scanner drive the SCIP indexer, so the two agree on what is a
//! link.

use tower_lsp_server::ls_types::{DocumentLink, Uri};

use crate::deb822::completion::{FieldContent, FieldInfo};
use crate::links::find_urls;
use crate::position::Source;
use text_size::{TextRange, TextSize};

/// Build a document link for the URL spanning `[start, end)` (byte offsets into
/// the source), labelling its tooltip with `field_name`. Returns `None` if the
/// span does not parse as a URI.
fn link(
    url: &str,
    start: usize,
    end: usize,
    field_name: &str,
    src: Source<'_>,
) -> Option<DocumentLink> {
    let uri = url.parse::<Uri>().ok()?;
    let range = src.text_range_to_lsp_range(TextRange::new(
        TextSize::new(start as u32),
        TextSize::new(end as u32),
    ));
    Some(DocumentLink {
        range,
        target: Some(uri),
        tooltip: Some(field_name.to_owned()),
        data: None,
    })
}

/// Document links for every URL-bearing field in a deb822 document. `fields`
/// classifies each field as URL-valued, prose, or plain; only the first two
/// yield links.
pub fn get_document_links(
    deb822: &deb822_lossless::Deb822,
    fields: &[FieldInfo],
    src: Source<'_>,
) -> Vec<DocumentLink> {
    let mut links = Vec::new();
    for para in deb822.paragraphs() {
        for entry in para.entries() {
            let Some(key) = entry.key() else { continue };
            let content = crate::deb822::completion::field_content(fields, &key);
            if content == FieldContent::Plain {
                continue;
            }
            let Some(vr) = entry.value_range() else {
                continue;
            };
            let start: usize = vr.start().into();
            let end: usize = vr.end().into();
            let value = &src.text[start..end];
            match content {
                FieldContent::Url => {
                    let trimmed = value.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let offset = value.find(trimmed).unwrap_or(0);
                    if let Some(l) = link(
                        trimmed,
                        start + offset,
                        start + offset + trimmed.len(),
                        &key,
                        src,
                    ) {
                        links.push(l);
                    }
                }
                FieldContent::Prose => {
                    for (rs, re) in find_urls(value) {
                        if let Some(l) = link(&value[rs..re], start + rs, start + re, &key, src) {
                            links.push(l);
                        }
                    }
                }
                FieldContent::Plain => unreachable!(),
            }
        }
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::fields::CONTROL_FIELDS;
    use crate::copyright::fields::COPYRIGHT_FIELDS;
    use crate::position::LineIndex;

    fn links(text: &str, fields: &[FieldInfo]) -> Vec<DocumentLink> {
        let deb822 = deb822_lossless::Deb822::parse(text).to_result().unwrap();
        let idx = LineIndex::new(text);
        get_document_links(&deb822, fields, Source::new(text, &idx))
    }

    #[test]
    fn links_homepage_whole_value() {
        let ls = links("Homepage: https://example.org/hello\n", CONTROL_FIELDS);
        assert_eq!(ls.len(), 1);
        assert_eq!(
            ls[0].target.as_ref().unwrap().as_str(),
            "https://example.org/hello"
        );
        assert_eq!(ls[0].tooltip.as_deref(), Some("Homepage"));
    }

    #[test]
    fn scans_prose_description() {
        let ls = links(
            "Description: a tool\n See https://example.org/x for details.\n",
            CONTROL_FIELDS,
        );
        assert_eq!(ls.len(), 1);
        assert_eq!(
            ls[0].target.as_ref().unwrap().as_str(),
            "https://example.org/x"
        );
    }

    #[test]
    fn ignores_plain_field() {
        let ls = links(
            "Maintainer: Jelmer <https://example.org/me>\n",
            CONTROL_FIELDS,
        );
        assert!(ls.is_empty());
    }

    #[test]
    fn links_copyright_format_and_scans_comment() {
        let ls = links(
            "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\nComment: derived from https://upstream.example/notice\n",
            COPYRIGHT_FIELDS,
        );
        let targets: Vec<_> = ls
            .iter()
            .map(|l| l.target.as_ref().unwrap().as_str())
            .collect();
        assert!(
            targets.contains(&"https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/")
        );
        assert!(targets.contains(&"https://upstream.example/notice"));
    }
}
