use tower_lsp_server::ls_types::Uri;

pub fn is_triggers_file(uri: &Uri) -> bool {
    let path = uri.as_str();
    path.ends_with("/debian/triggers")
        || path.ends_with("/DEBIAN/triggers")
        || path.ends_with(".triggers")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    #[test]
    fn detects_qualified_and_unqualified() {
        assert!(is_triggers_file(&uri("file:///p/debian/triggers")));
        assert!(is_triggers_file(&uri("file:///p/debian/mypkg.triggers")));
        assert!(is_triggers_file(&uri("file:///p/DEBIAN/triggers")));
    }

    #[test]
    fn rejects_other_files() {
        assert!(!is_triggers_file(&uri("file:///p/debian/control")));
        assert!(!is_triggers_file(&uri("file:///p/triggers")));
        assert!(!is_triggers_file(&uri("file:///p/debian/triggers.bak")));
    }
}
