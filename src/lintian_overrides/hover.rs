use tower_lsp_server::ls_types::{Hover, HoverContents, MarkupContent, MarkupKind};

use lintian_overrides::SyntaxKind;

/// Get hover information for a lintian-overrides file at the given cursor position.
pub fn get_hover(
    kind: SyntaxKind,
    text: &str,
    tags: &[(String, String)],
    packages: &[String],
) -> Option<Hover> {
    let value = match kind {
        SyntaxKind::TAG => {
            let description = tags
                .iter()
                .find(|(tag, _)| tag == text)
                .map(|(_, desc)| desc.as_str())?;
            if description.is_empty() {
                return None;
            }
            format!("**{}**\n\nLintian tag\n\n{}", text, description)
        }
        SyntaxKind::PACKAGE_NAME => {
            if packages.iter().any(|p| p == text) {
                format!("**{}**\n\nPackage defined in `debian/control`", text)
            } else {
                format!("**{}**\n\nPackage, not found in `debian/control`", text)
            }
        }
        SyntaxKind::ARCH => {
            let bare = text.trim_start_matches('!');
            if text.starts_with('!') {
                format!(
                    "**{}**\n\nArchitecture restriction (excludes `{}`)",
                    text, bare
                )
            } else {
                format!("**{}**\n\nArchitecture restriction", text)
            }
        }
        SyntaxKind::PACKAGE_TYPE => {
            let desc = match text {
                "source" => "Applies to the source package.",
                "binary" => "Applies to binary packages.",
                "udeb" => "Applies to udeb (micro-deb) packages.",
                _ => return None,
            };
            format!("**{}**\n\nPackage type\n\n{}", text, desc)
        }
        _ => return None,
    };

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: None,
    })
}
#[cfg(test)]
mod tests {
    use super::*;

    fn tags() -> Vec<(String, String)> {
        vec![
            (
                "hardening-no-pie".to_string(),
                "Binary is not built with PIE.".to_string(),
            ),
            ("empty-desc".to_string(), String::new()),
        ]
    }

    fn packages() -> Vec<String> {
        vec!["libcurl4".to_string(), "libcurl".to_string()]
    }

    fn md(h: &Hover) -> &str {
        match &h.contents {
            HoverContents::Markup(m) => &m.value,
            _ => panic!("expected markup"),
        }
    }

    #[test]
    fn test_hover_tag() {
        let h = get_hover(SyntaxKind::TAG, "hardening-no-pie", &tags(), &packages())
            .expect("should have hover");
        assert!(md(&h).contains("Lintian tag"));
        assert!(md(&h).contains("Binary is not built with PIE."));
    }

    #[test]
    fn test_hover_unknown_tag() {
        assert!(get_hover(SyntaxKind::TAG, "nope", &tags(), &packages()).is_none());
    }

    #[test]
    fn test_hover_tag_empty_description() {
        assert!(get_hover(SyntaxKind::TAG, "empty-desc", &tags(), &packages()).is_none());
    }

    #[test]
    fn test_hover_known_package() {
        let h = get_hover(SyntaxKind::PACKAGE_NAME, "libcurl4", &tags(), &packages()).unwrap();
        assert!(md(&h).contains("debian/control"));
        assert!(!md(&h).contains("not found"));
    }

    #[test]
    fn test_hover_unknown_package() {
        let h = get_hover(SyntaxKind::PACKAGE_NAME, "ghost", &tags(), &packages()).unwrap();
        assert!(md(&h).contains("not found"));
    }

    #[test]
    fn test_hover_arch() {
        let h = get_hover(SyntaxKind::ARCH, "amd64", &tags(), &packages()).unwrap();
        assert!(md(&h).contains("Architecture restriction"));
        assert!(!md(&h).contains("excludes"));
    }

    #[test]
    fn test_hover_arch_negated() {
        let h = get_hover(SyntaxKind::ARCH, "!amd64", &tags(), &packages()).unwrap();
        assert!(md(&h).contains("excludes"));
        assert!(md(&h).contains("amd64"));
    }

    #[test]
    fn test_hover_type_binary() {
        let h = get_hover(SyntaxKind::PACKAGE_TYPE, "binary", &tags(), &packages()).unwrap();
        assert!(md(&h).contains("binary packages"));
    }

    #[test]
    fn test_hover_type_unknown_returns_none() {
        assert!(get_hover(SyntaxKind::PACKAGE_TYPE, "weird", &tags(), &packages()).is_none());
    }
}
