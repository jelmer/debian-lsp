use tower_lsp_server::ls_types::Uri;

/// Whether the URI points to a debconf templates file.
///
/// Matches:
/// - `debian/templates`
/// - `debian/<package>.templates`
pub fn is_templates_file(uri: &Uri) -> bool {
    let path = uri.as_str();
    if path.ends_with("/debian/templates") {
        return true;
    }
    match path.strip_suffix(".templates") {
        Some(rest) => rest
            .rsplit_once('/')
            .is_some_and(|(dir, _)| dir.ends_with("/debian")),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    #[test]
    fn detects_qualified_and_unqualified() {
        assert!(is_templates_file(&uri("file:///p/debian/templates")));
        assert!(is_templates_file(&uri("file:///p/debian/mypkg.templates")));
    }

    #[test]
    fn rejects_other_files() {
        assert!(!is_templates_file(&uri("file:///p/debian/control")));
        assert!(!is_templates_file(&uri("file:///p/templates")));
        assert!(!is_templates_file(&uri("file:///p/foo.templates")));
        assert!(!is_templates_file(&uri("file:///p/debian/templates.bak")));
    }
}
