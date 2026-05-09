use tower_lsp_server::ls_types::Uri;

/// Check if a given URI represents a debcargo.toml file.
pub fn is_debcargo_toml(uri: &Uri) -> bool {
    let path = uri.as_str();
    path.ends_with("/debian/debcargo.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_debcargo_toml() {
        let valid = [
            "file:///path/to/debian/debcargo.toml",
            "file:///project/debian/debcargo.toml",
        ];
        let invalid = [
            "file:///path/to/debcargo.toml",
            "file:///path/to/debian/control",
            "file:///path/to/debian/debcargo.toml.bak",
            "file:///path/to/Cargo.toml",
        ];

        for path in valid {
            let uri = path.parse::<Uri>().unwrap();
            assert_eq!(is_debcargo_toml(&uri), true, "should detect: {path}");
        }
        for path in invalid {
            let uri = path.parse::<Uri>().unwrap();
            assert_eq!(is_debcargo_toml(&uri), false, "should not detect: {path}");
        }
    }
}
